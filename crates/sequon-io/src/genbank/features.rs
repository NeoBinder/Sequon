//! FEATURES block parsing.
//!
//! This module implements the line classification, qualifier grammar and
//! location assembly rules in docs/design/genbank-adapter.md section 4. A
//! feature whose location cannot be represented is skipped and recorded in
//! dropped[]; a structurally broken block aborts the record.

use sequon_model::{Feature, Qualifier, Topology};

use crate::error::ImportError;
use crate::report::{Diagnostic, Dropped, Severity, SourcePosition};

use super::location::{self, LocationError, RawLocation};
use super::{qualifier, ParseContext, ParseFailure};
use crate::so;

struct RawQualifier {
    text: String,
    line: u32,
}

struct RawFeature {
    key: String,
    location_text: String,
    location_col: u32,
    qualifiers: Vec<RawQualifier>,
    line: u32,
}

struct ParsedFeature {
    key: String,
    so_term: Option<String>,
    name: Option<String>,
    raw_location: RawLocation,
    qualifiers: Vec<Qualifier>,
    line: u32,
    location_text: String,
}

/// Stateful FEATURES block parser for one record.
pub(crate) struct FeatureState {
    current: Option<RawFeature>,
    finished: Vec<ParsedFeature>,
    started: u64,
}

impl FeatureState {
    /// Creates an empty block parser.
    pub fn new() -> Self {
        Self {
            current: None,
            finished: Vec::new(),
            started: 0,
        }
    }

    /// Consumes one physical FEATURES line.
    pub fn push_line(
        &mut self,
        line: &str,
        line_no: u32,
        context: &mut ParseContext<'_>,
        diagnostics: &mut Vec<Diagnostic>,
        dropped: &mut Vec<Dropped>,
    ) -> Result<(), ParseFailure> {
        let Some(first) = first_non_space_char(line) else {
            return Ok(());
        };
        let total = line.chars().count();

        if slice_chars(line, first, total).starts_with('/') {
            let Some(current) = self.current.as_mut() else {
                return Err(features_malformed(line_no));
            };
            let text = slice_chars(line, first, total).trim_end();
            if text.len() <= 1 {
                return Err(features_malformed(line_no));
            }
            current.qualifiers.push(RawQualifier {
                text: text[1..].to_string(),
                line: line_no,
            });
            return Ok(());
        }

        if first < 20 {
            self.finish_current(context, diagnostics, dropped)?;

            self.started = self.started.saturating_add(1);
            if self.started > context.limits.max_features {
                return Err(ParseFailure::Fatal(ImportError::LimitExceeded {
                    limit: "max_features",
                    limit_value: context.limits.max_features,
                    actual: self.started,
                    position: Some(position(line_no)),
                }));
            }

            let key_start = first.min(5);
            let token_end = line
                .chars()
                .enumerate()
                .skip(key_start)
                .find(|(_, character)| character.is_whitespace())
                .map(|(index, _)| index)
                .unwrap_or(total);
            let (key, location_start) = if token_end > 20 {
                (
                    slice_chars(line, key_start, token_end).trim().to_string(),
                    token_end,
                )
            } else {
                (
                    slice_chars(line, key_start, total.min(20))
                        .trim()
                        .to_string(),
                    total.min(20),
                )
            };
            if key.is_empty() {
                return Err(features_malformed(line_no));
            }

            let remainder = slice_chars(line, location_start, total);
            let location_text = remainder.trim().to_string();
            let leading = remainder.len() - remainder.trim_start().len();
            let location_col = u32::try_from(location_start + leading + 1).unwrap_or(u32::MAX);

            if first != 5 && !*context.column_misaligned_emitted {
                diagnostics.push(
                    Diagnostic::new(
                        "GB_FEATURE_COLUMN_MISALIGNED",
                        Severity::Info,
                        "FEATURES columns drifted from the standard offsets; the line was parsed tolerantly",
                    )
                    .with_position(SourcePosition::Genbank {
                        line: line_no,
                        column: Some(u32::try_from(first + 1).unwrap_or(u32::MAX)),
                    }),
                );
                *context.column_misaligned_emitted = true;
            }

            self.current = Some(RawFeature {
                key,
                location_text,
                location_col,
                qualifiers: Vec::new(),
                line: line_no,
            });
            return Ok(());
        }

        let Some(current) = self.current.as_mut() else {
            return Err(features_malformed(line_no));
        };
        let text = line.trim();
        if text.is_empty() {
            return Ok(());
        }
        if let Some(last) = current.qualifiers.last_mut() {
            if !last.text.contains('=') {
                return Err(features_malformed(line_no));
            }
            last.text.push(' ');
            last.text.push_str(text);
            let guard = context
                .limits
                .max_qualifier_value_bytes
                .saturating_add(4096);
            if last.text.len() as u64 > guard {
                return Err(ParseFailure::Fatal(ImportError::LimitExceeded {
                    limit: "max_qualifier_value_bytes",
                    limit_value: context.limits.max_qualifier_value_bytes,
                    actual: last.text.len() as u64,
                    position: Some(position(line_no)),
                }));
            }
        } else {
            current.location_text.push(' ');
            current.location_text.push_str(text);
        }
        Ok(())
    }

    /// Finalizes the current feature at a section boundary.
    pub fn finish_section(
        &mut self,
        context: &mut ParseContext<'_>,
        diagnostics: &mut Vec<Diagnostic>,
        dropped: &mut Vec<Dropped>,
    ) -> Result<(), ParseFailure> {
        self.finish_current(context, diagnostics, dropped)
    }

    /// Converts syntactically parsed features after ORIGIN is known.
    pub fn finalize(
        self,
        length: u64,
        topology: Topology,
        diagnostics: &mut Vec<Diagnostic>,
        dropped: &mut Vec<Dropped>,
    ) -> Vec<Feature> {
        let mut features = Vec::new();
        for parsed in self.finished {
            let location = match location::normalize(&parsed.raw_location, topology, length) {
                Ok(location) => location,
                Err(()) => {
                    diagnostics.push(
                        Diagnostic::new(
                            "GB_COORDINATE_OUT_OF_RANGE",
                            Severity::Warning,
                            "feature coordinate is outside the sequence; the feature was skipped",
                        )
                        .with_position(position(parsed.line)),
                    );
                    dropped.push(feature_dropped(
                        &parsed.key,
                        &parsed.location_text,
                        "coordinate_out_of_range",
                        parsed.line,
                    ));
                    continue;
                }
            };

            if location
                .segments
                .iter()
                .any(|segment| segment.start_partial || segment.end_partial)
            {
                diagnostics.push(
                    Diagnostic::new(
                        "GB_LOCATION_PARTIAL_BOUNDARY",
                        Severity::Info,
                        "partial boundary marker was preserved",
                    )
                    .with_position(position(parsed.line)),
                );
            }
            if location
                .segments
                .iter()
                .any(|segment| segment.start == segment.end)
            {
                diagnostics.push(
                    Diagnostic::new(
                        "GB_LOCATION_BETWEEN_POSITIONS",
                        Severity::Info,
                        "between-positions site was mapped to a zero-length segment",
                    )
                    .with_position(position(parsed.line)),
                );
            }

            features.push(Feature {
                key: parsed.key,
                so_term: parsed.so_term,
                name: parsed.name,
                location,
                qualifiers: parsed.qualifiers,
            });
        }
        features
    }

    fn finish_current(
        &mut self,
        context: &ParseContext<'_>,
        diagnostics: &mut Vec<Diagnostic>,
        dropped: &mut Vec<Dropped>,
    ) -> Result<(), ParseFailure> {
        let Some(raw) = self.current.take() else {
            return Ok(());
        };

        let mut qualifiers = Vec::with_capacity(raw.qualifiers.len());
        for qualifier in &raw.qualifiers {
            qualifiers.push(qualifier::parse(
                &qualifier.text,
                qualifier.line,
                context.limits,
                diagnostics,
            )?);
        }

        if raw.location_text.is_empty() {
            return Err(ParseFailure::Record(ImportError::Malformed {
                code: "GB_LOCATION_MALFORMED",
                message: "feature has no location".to_string(),
                position: Some(SourcePosition::Genbank {
                    line: raw.line,
                    column: Some(raw.location_col),
                }),
            }));
        }

        let raw_location = match location::parse(
            &raw.location_text,
            raw.line,
            raw.location_col,
            context.limits,
        ) {
            Ok(location) => location,
            Err(LocationError::Unsupported { message, .. }) => {
                diagnostics.push(
                    Diagnostic::new(
                        "GB_LOCATION_UNSUPPORTED",
                        Severity::Warning,
                        format!("{message}; the feature was skipped"),
                    )
                    .with_position(position(raw.line)),
                );
                dropped.push(feature_dropped(
                    &raw.key,
                    &raw.location_text,
                    "unsupported_location",
                    raw.line,
                ));
                return Ok(());
            }
            Err(LocationError::Malformed { message, column }) => {
                return Err(ParseFailure::Record(ImportError::Malformed {
                    code: "GB_LOCATION_MALFORMED",
                    message,
                    position: Some(SourcePosition::Genbank {
                        line: raw.line,
                        column: Some(column),
                    }),
                }))
            }
            Err(LocationError::Fatal(error)) => return Err(ParseFailure::Fatal(error)),
        };

        let name = qualifiers
            .iter()
            .find(|qualifier| qualifier.key == "gene")
            .and_then(|qualifier| qualifier.value.as_ref())
            .filter(|value| !value.is_empty())
            .cloned();
        let so_term = so::so_term(&raw.key).map(str::to_string);
        if so_term.is_none() {
            diagnostics.push(
                Diagnostic::new(
                    "GB_UNKNOWN_FEATURE_KEY",
                    Severity::Info,
                    format!(
                        "feature key '{}' is not in the static SO table; it was kept verbatim",
                        raw.key
                    ),
                )
                .with_position(position(raw.line)),
            );
        }

        self.finished.push(ParsedFeature {
            key: raw.key,
            so_term,
            name,
            raw_location,
            qualifiers,
            line: raw.line,
            location_text: raw.location_text,
        });
        Ok(())
    }
}

fn feature_dropped(key: &str, location_text: &str, reason: &str, line: u32) -> Dropped {
    Dropped {
        what: "feature".to_string(),
        reason: reason.to_string(),
        record_index: None,
        position: Some(position(line)),
        packet_type: None,
        offset: None,
        length: None,
        digest: None,
        feature_key: Some(key.to_string()),
        location_text: Some(location_text.to_string()),
    }
}

fn features_malformed(line: u32) -> ParseFailure {
    ParseFailure::Record(ImportError::Malformed {
        code: "GB_FEATURES_MALFORMED",
        message: "the FEATURES block is structurally broken".to_string(),
        position: Some(position(line)),
    })
}

fn first_non_space_char(line: &str) -> Option<usize> {
    line.chars()
        .position(|character| !character.is_whitespace())
}

fn char_to_byte(line: &str, char_index: usize) -> usize {
    line.char_indices()
        .nth(char_index)
        .map(|(byte, _)| byte)
        .unwrap_or(line.len())
}

fn slice_chars(line: &str, start: usize, end: usize) -> &str {
    if start >= end {
        return "";
    }
    let start_byte = char_to_byte(line, start);
    let end_byte = char_to_byte(line, end);
    line.get(start_byte..end_byte).unwrap_or("")
}

fn position(line: u32) -> SourcePosition {
    SourcePosition::Genbank { line, column: None }
}
