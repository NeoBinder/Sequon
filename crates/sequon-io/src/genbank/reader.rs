//! GenBank reader: boundaries, record state machine, report assembly.

use std::collections::BTreeMap;

use sequon_model::{
    DigestValue, Digests, Extras, JsonValue, Producer, ProducerKind, Provenance, Record,
    SourceFormat, DIGEST_ALGORITHM_V1, SCHEMA_ID,
};

use crate::api::{ImportOptions, ImportOutcome, ImportResult, MultiplePolicy};
use crate::error::ImportError;
use crate::report::{Diagnostic, Dropped, ImportReport, RecordSummary, Severity, SourcePosition};

use super::features::FeatureState;
use super::header::{self, Header, HeaderBuilder, Locus};
use super::origin::OriginBuilder;
use super::{line_number, ParseContext, ParseFailure};

/// Parses one GenBank input.
pub(crate) fn import(bytes: &[u8], options: &ImportOptions) -> ImportResult {
    if bytes.len() as u64 > options.limits.max_input_bytes {
        return Err(ImportError::LimitExceeded {
            limit: "max_input_bytes",
            limit_value: options.limits.max_input_bytes,
            actual: bytes.len() as u64,
            position: None,
        });
    }

    let text = std::str::from_utf8(bytes).map_err(|error| {
        let line = bytes[..error.valid_up_to()]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count()
            .saturating_add(1);
        ImportError::Malformed {
            code: "GB_ENCODING_INVALID",
            message: "input is not valid UTF-8".to_string(),
            position: Some(SourcePosition::Genbank {
                line: u32::try_from(line).unwrap_or(u32::MAX),
                column: None,
            }),
        }
    })?;

    let (lines, crlf_normalized) = split_lines(text);
    let Some(first) = lines.iter().position(|line| !line.trim().is_empty()) else {
        return Err(empty_input());
    };

    if !is_locus_line(lines[first]) {
        let later = lines
            .iter()
            .enumerate()
            .skip(first)
            .find(|(_, line)| is_locus_line(line))
            .map(|(index, _)| index);
        return match later {
            Some(_) => Err(content_before_locus(line_number(first))),
            None => Err(missing_locus(line_number(first))),
        };
    }

    let source_digest = sequon_algo::bytes_digest(bytes);
    let source_bytes = bytes.len() as u64;
    let parser = Producer {
        kind: ProducerKind::Parser,
        name: "sequon-io".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };

    let mut state = FileParser {
        lines,
        options,
        source_digest: source_digest.clone(),
        source_bytes,
        column_misaligned: false,
    };

    let mut results: Vec<(u32, RecordStep)> = Vec::new();
    let mut index = first;
    let mut ordinal = 0_u32;
    loop {
        while index < state.lines.len() && state.lines[index].trim().is_empty() {
            index += 1;
        }
        if index >= state.lines.len() {
            break;
        }
        if !is_locus_line(state.lines[index]) {
            return Err(content_before_locus(line_number(index)));
        }
        if ordinal > 0 && state.options.multiple == MultiplePolicy::Reject {
            return Err(ImportError::Malformed {
                code: "GB_MULTIPLE_RECORDS_NOT_ALLOWED",
                message: "input holds more than one record while the policy is Reject".to_string(),
                position: Some(position(line_number(index))),
            });
        }
        let next_ordinal = u64::from(ordinal).saturating_add(1);
        if next_ordinal > state.options.limits.max_records {
            return Err(ImportError::LimitExceeded {
                limit: "max_records",
                limit_value: state.options.limits.max_records,
                actual: next_ordinal,
                position: Some(position(line_number(index))),
            });
        }

        match state.parse_record(index) {
            RecordStep::Success {
                record,
                summary,
                diagnostics,
                dropped,
                next,
            } => {
                results.push((
                    ordinal,
                    RecordStep::Success {
                        record,
                        summary,
                        diagnostics,
                        dropped,
                        next,
                    },
                ));
                index = next;
                ordinal = ordinal.saturating_add(1);
            }
            RecordStep::Failure { error, next } => {
                if state.options.multiple == MultiplePolicy::First && ordinal == 0 {
                    return Err(error);
                }
                results.push((ordinal, RecordStep::Failure { error, next }));
                ordinal = ordinal.saturating_add(1);
                match next {
                    Some(next) => index = next,
                    None => break,
                }
            }
            RecordStep::Fatal(error) => return Err(error),
        }
    }

    assemble_outcome(
        results,
        crlf_normalized,
        source_digest,
        source_bytes,
        parser,
        options,
    )
}

fn assemble_outcome(
    results: Vec<(u32, RecordStep)>,
    crlf_normalized: bool,
    source_digest: DigestValue,
    source_bytes: u64,
    parser: Producer,
    options: &ImportOptions,
) -> ImportResult {
    let successes = results
        .iter()
        .filter(|(_, step)| matches!(step, RecordStep::Success { .. }))
        .count();
    if successes == 0 {
        for (_, step) in &results {
            if let RecordStep::Failure { error, .. } = step {
                return Err(error.clone());
            }
        }
        return Err(missing_locus(1));
    }

    let first_record_only = options.multiple == MultiplePolicy::First;
    let mut records = Vec::new();
    let mut report = ImportReport::new(
        SourceFormat::Genbank,
        source_digest,
        source_bytes,
        parser,
        None,
    );

    if crlf_normalized {
        report.diagnostics.push(Diagnostic::new(
            "GB_CRLF_NORMALIZED",
            Severity::Info,
            "CRLF line endings were normalized to LF before parsing",
        ));
    }

    for (ordinal, step) in results {
        match step {
            RecordStep::Success {
                record,
                mut summary,
                mut diagnostics,
                mut dropped,
                ..
            } => {
                summary.index = ordinal;
                for diagnostic in &mut diagnostics {
                    diagnostic.record_index = Some(ordinal);
                }
                for entry in &mut dropped {
                    entry.record_index = Some(ordinal);
                }
                report.diagnostics.extend(diagnostics);
                report.dropped.extend(dropped);
                if !first_record_only || ordinal == 0 {
                    report.records.push(summary);
                    records.push(*record);
                }
            }
            RecordStep::Failure { error, next: _ } => {
                report.diagnostics.push(error_diagnostic(&error, ordinal));
            }
            RecordStep::Fatal(error) => return Err(error),
        }
    }

    report.sort_diagnostics();
    Ok(ImportOutcome { records, report })
}

enum RecordStep {
    Success {
        record: Box<Record>,
        summary: RecordSummary,
        diagnostics: Vec<Diagnostic>,
        dropped: Vec<Dropped>,
        next: usize,
    },
    Failure {
        error: ImportError,
        next: Option<usize>,
    },
    Fatal(ImportError),
}

struct FileParser<'a> {
    lines: Vec<&'a str>,
    options: &'a ImportOptions,
    source_digest: DigestValue,
    source_bytes: u64,
    column_misaligned: bool,
}

enum Section {
    Header,
    Features,
    Origin,
}

impl FileParser<'_> {
    fn parse_record(&mut self, start: usize) -> RecordStep {
        let mut diagnostics = Vec::new();
        let mut dropped = Vec::new();
        let locus_line = line_number(start);
        let locus = match header::parse_locus(
            self.lines[start],
            locus_line,
            self.options,
            &mut diagnostics,
        ) {
            Ok(locus) => locus,
            Err(error) => return failure_from(&self.lines, error, start),
        };

        let mut header_builder = HeaderBuilder::new(self.options.limits.max_header_bytes);
        let mut features = FeatureState::new();
        let mut header: Option<Header> = None;
        let mut origin: Option<OriginBuilder> = None;
        let mut section = Section::Header;
        let mut index = start.saturating_add(1);
        let mut context = ParseContext {
            limits: &self.options.limits,
            column_misaligned_emitted: &mut self.column_misaligned,
        };

        loop {
            if index >= self.lines.len() {
                return RecordStep::Failure {
                    error: unterminated(last_content_line(&self.lines)),
                    next: None,
                };
            }
            let line = self.lines[index];
            let line_no = line_number(index);

            if is_locus_line(line) {
                return RecordStep::Failure {
                    error: unterminated(locus_line),
                    next: None,
                };
            }

            if is_terminator(line) {
                if matches!(section, Section::Origin) {
                    let sequence = match origin.take().unwrap_or_else(OriginBuilder::new).finish(
                        locus.declared_length,
                        locus.line,
                        &mut diagnostics,
                    ) {
                        Ok(sequence) => sequence,
                        Err(error) => {
                            return RecordStep::Failure {
                                error,
                                next: Some(index.saturating_add(1)),
                            }
                        }
                    };
                    let features = features.finalize(
                        sequence.len() as u64,
                        locus.topology,
                        &mut diagnostics,
                        &mut dropped,
                    );
                    let header = header.take().unwrap_or_default();
                    let (record, summary) = match build_record(
                        &locus,
                        &header,
                        sequence,
                        features,
                        &self.source_digest,
                        self.source_bytes,
                    ) {
                        Ok(built) => built,
                        Err(error) => {
                            return RecordStep::Failure {
                                error,
                                next: Some(index.saturating_add(1)),
                            }
                        }
                    };
                    return RecordStep::Success {
                        record: Box::new(record),
                        summary,
                        diagnostics,
                        dropped,
                        next: index.saturating_add(1),
                    };
                }
                return RecordStep::Failure {
                    error: missing_origin(),
                    next: Some(index.saturating_add(1)),
                };
            }

            match section {
                Section::Header => {
                    if is_section_keyword(line, "FEATURES") {
                        header = Some(header_builder.finish());
                        section = Section::Features;
                        index = index.saturating_add(1);
                    } else if is_section_keyword(line, "ORIGIN") {
                        header = Some(header_builder.finish());
                        origin = Some(OriginBuilder::new());
                        section = Section::Origin;
                        index = index.saturating_add(1);
                    } else if let Err(error) =
                        header_builder.push_line(line, line_no, &mut diagnostics)
                    {
                        return RecordStep::Fatal(error);
                    } else {
                        index = index.saturating_add(1);
                    }
                }
                Section::Features => {
                    if is_section_keyword(line, "ORIGIN") {
                        match features.finish_section(&mut context, &mut diagnostics, &mut dropped)
                        {
                            Ok(()) => {}
                            Err(ParseFailure::Record(error)) => {
                                return RecordStep::Failure {
                                    error,
                                    next: find_terminator(&self.lines, index),
                                }
                            }
                            Err(ParseFailure::Fatal(error)) => return RecordStep::Fatal(error),
                        }
                        origin = Some(OriginBuilder::new());
                        section = Section::Origin;
                        index = index.saturating_add(1);
                    } else if is_known_other_section(line) {
                        return RecordStep::Failure {
                            error: missing_origin(),
                            next: find_terminator(&self.lines, index),
                        };
                    } else {
                        match features.push_line(
                            line,
                            line_no,
                            &mut context,
                            &mut diagnostics,
                            &mut dropped,
                        ) {
                            Ok(()) => index = index.saturating_add(1),
                            Err(ParseFailure::Record(error)) => {
                                return RecordStep::Failure {
                                    error,
                                    next: find_terminator(&self.lines, index),
                                }
                            }
                            Err(ParseFailure::Fatal(error)) => return RecordStep::Fatal(error),
                        }
                    }
                }
                Section::Origin => {
                    let Some(builder) = origin.as_mut() else {
                        return RecordStep::Fatal(missing_origin());
                    };
                    match builder.push_line(line, line_no, &self.options.limits) {
                        Ok(()) => index = index.saturating_add(1),
                        Err(error @ ImportError::LimitExceeded { .. }) => {
                            return RecordStep::Fatal(error)
                        }
                        Err(error) => {
                            return RecordStep::Failure {
                                error,
                                next: find_terminator(&self.lines, index),
                            }
                        }
                    }
                }
            }
        }
    }
}

fn build_record(
    locus: &Locus,
    header: &Header,
    sequence: String,
    features: Vec<sequon_model::Feature>,
    source_digest: &DigestValue,
    source_bytes: u64,
) -> Result<(Record, RecordSummary), ImportError> {
    let sequence_digest =
        sequon_algo::sequence_digest(&sequence).map_err(|_| ImportError::Malformed {
            code: "GB_ORIGIN_INVALID_CHAR",
            message: "normalized sequence failed the digest precondition".to_string(),
            position: None,
        })?;
    let canonical_digest =
        sequon_algo::canonical_digest(&sequence, locus.topology).map_err(|_| {
            ImportError::Malformed {
                code: "GB_ORIGIN_INVALID_CHAR",
                message: "normalized sequence failed the digest precondition".to_string(),
                position: None,
            }
        })?;
    let record = Record {
        schema: SCHEMA_ID.to_string(),
        kind: locus.kind,
        molecule_type: locus.molecule_type,
        topology: locus.topology,
        name: locus.name.clone(),
        description: header.description.clone(),
        length: sequence.len() as u64,
        sequence,
        digests: Digests {
            algorithm: DIGEST_ALGORITHM_V1.to_string(),
            sequence: sequence_digest.clone(),
            canonical: canonical_digest.clone(),
        },
        features,
        provenance: Provenance {
            producer: Producer {
                kind: ProducerKind::Parser,
                name: "sequon-io".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            source_format: SourceFormat::Genbank,
            source_digest: source_digest.clone(),
            source_bytes,
            export_version: None,
        },
        extras: build_extras(header, locus.division.as_deref()),
    };

    record.validate().map_err(|error| ImportError::Malformed {
        code: "RECORD_FIELD_INVALID",
        message: format!("record validation failed: {error}"),
        position: Some(position(locus.line)),
    })?;

    let summary = RecordSummary {
        index: 0,
        name: record.name.clone(),
        kind: record.kind,
        length: record.length,
        sequence_digest,
        canonical_digest,
    };
    Ok((record, summary))
}

fn build_extras(header: &Header, division: Option<&str>) -> Extras {
    let mut payload = BTreeMap::new();
    let entries = header
        .entries
        .iter()
        .map(|entry| {
            let mut object = BTreeMap::new();
            object.insert("key".to_string(), JsonValue::from(entry.key.clone()));
            object.insert("value".to_string(), JsonValue::from(entry.value.clone()));
            JsonValue::Object(object)
        })
        .collect();
    payload.insert("header".to_string(), JsonValue::Array(entries));
    if let Some(division) = division {
        payload.insert("division".to_string(), JsonValue::from(division));
    }
    let mut extras = Extras::new();
    extras.insert("genbank", JsonValue::Object(payload));
    extras
}

fn split_lines(text: &str) -> (Vec<&str>, bool) {
    let mut lines = Vec::new();
    let mut normalized = false;
    let mut start = 0;
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            let mut line = &text[start..index];
            if let Some(stripped) = line.strip_suffix('\r') {
                line = stripped;
                normalized = true;
            }
            lines.push(line);
            start = index.saturating_add(1);
        }
    }
    let mut last = &text[start..];
    if let Some(stripped) = last.strip_suffix('\r') {
        last = stripped;
        normalized = true;
    }
    lines.push(last);
    (lines, normalized)
}

fn is_locus_line(line: &str) -> bool {
    line == "LOCUS"
        || line.strip_prefix("LOCUS").is_some_and(|rest| {
            rest.chars()
                .next()
                .is_some_and(|character| character.is_whitespace())
        })
}

fn is_terminator(line: &str) -> bool {
    line.trim_end() == "//"
}

fn is_known_other_section(line: &str) -> bool {
    is_section_keyword(line, "CONTIG")
        || is_section_keyword(line, "BASE")
        || is_section_keyword(line, "FEATURES")
}

fn last_content_line(lines: &[&str]) -> u32 {
    lines
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .map(line_number)
        .unwrap_or(1)
}

fn is_section_keyword(line: &str, keyword: &str) -> bool {
    line.starts_with(keyword)
        && line[keyword.len()..]
            .chars()
            .next()
            .is_none_or(|character| character.is_whitespace())
}

fn find_terminator(lines: &[&str], from: usize) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .skip(from)
        .find(|(_, line)| is_terminator(line))
        .map(|(index, _)| index.saturating_add(1))
}

fn failure_from(lines: &[&str], error: ImportError, from: usize) -> RecordStep {
    RecordStep::Failure {
        error,
        next: find_terminator(lines, from),
    }
}

fn error_diagnostic(error: &ImportError, ordinal: u32) -> Diagnostic {
    Diagnostic {
        code: error.code().to_string(),
        severity: Severity::Error,
        message: error_message(error),
        position: error.position().cloned(),
        record_index: Some(ordinal),
    }
}

fn error_message(error: &ImportError) -> String {
    match error {
        ImportError::Malformed { message, .. } | ImportError::Unsupported { message, .. } => {
            message.clone()
        }
        ImportError::MissingRequired { field, .. } => {
            format!("required record field '{}' has no source", field.as_str())
        }
        ImportError::LimitExceeded { limit, .. } => {
            format!("configured limit '{limit}' was exceeded")
        }
        ImportError::FormatUnknown { hint } => hint.clone(),
    }
}

fn empty_input() -> ImportError {
    ImportError::Malformed {
        code: "GB_EMPTY_INPUT",
        message: "input is empty".to_string(),
        position: None,
    }
}

fn missing_locus(line: u32) -> ImportError {
    ImportError::Malformed {
        code: "GB_MISSING_LOCUS",
        message: "no LOCUS line was found where a record should start".to_string(),
        position: Some(position(line)),
    }
}

fn content_before_locus(line: u32) -> ImportError {
    ImportError::Malformed {
        code: "GB_CONTENT_BEFORE_LOCUS",
        message: "non-empty content appears before the first LOCUS line".to_string(),
        position: Some(position(line)),
    }
}

fn missing_origin() -> ImportError {
    ImportError::Malformed {
        code: "GB_MISSING_ORIGIN",
        message: "record has no ORIGIN section".to_string(),
        position: None,
    }
}

fn unterminated(line: u32) -> ImportError {
    ImportError::Malformed {
        code: "GB_RECORD_UNTERMINATED",
        message: "record has no terminating // before end of input".to_string(),
        position: Some(position(line)),
    }
}

fn position(line: u32) -> SourcePosition {
    SourcePosition::Genbank { line, column: None }
}
