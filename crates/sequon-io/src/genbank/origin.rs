//! ORIGIN sequence parsing and normalization.
//!
//! ORIGIN line numbers and grouping spaces are formatting, not sequence. The
//! adapter strips them, uppercases bases, validates the IUPAC DNA alphabet and
//! reports the record's actual length.

use sequon_algo::IUPAC_DNA;

use crate::error::ImportError;
use crate::limits::Limits;
use crate::report::{Diagnostic, Severity, SourcePosition};

/// Accumulates one ORIGIN section.
pub(crate) struct OriginBuilder {
    sequence: String,
    normalized: bool,
}

impl OriginBuilder {
    /// Creates an empty builder.
    pub fn new() -> Self {
        Self {
            sequence: String::new(),
            normalized: false,
        }
    }

    /// Consumes one physical sequence line.
    pub fn push_line(
        &mut self,
        line: &str,
        line_no: u32,
        limits: &Limits,
    ) -> Result<(), ImportError> {
        let trimmed = line.trim_end();
        if trimmed.trim().is_empty() {
            return Ok(());
        }

        let mut rest = trimmed.trim_start();
        let digit_len = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digit_len > 0 {
            let after = &rest[digit_len..];
            let followed_by_space = after
                .chars()
                .next()
                .is_none_or(|character| character.is_ascii_whitespace());
            if followed_by_space {
                rest = after.trim_start();
            }
        }

        for character in rest.chars() {
            if character.is_ascii_whitespace() {
                continue;
            }
            let uppercase = character.to_ascii_uppercase();
            if uppercase == 'U' {
                return Err(ImportError::Malformed {
                    code: "GB_RNA_BASE_IN_DNA",
                    message: "a DNA record contains U; RNA input is not silently converted"
                        .to_string(),
                    position: Some(position(line_no)),
                });
            }
            if !IUPAC_DNA.contains(uppercase) {
                return Err(ImportError::Malformed {
                    code: "GB_ORIGIN_INVALID_CHAR",
                    message: "ORIGIN contains a character outside the IUPAC DNA alphabet"
                        .to_string(),
                    position: Some(position(line_no)),
                });
            }
            if character.is_ascii_lowercase() {
                self.normalized = true;
            }
            let next_len = self.sequence.len() as u64 + 1;
            if next_len > limits.max_sequence_len {
                return Err(ImportError::LimitExceeded {
                    limit: "max_sequence_len",
                    limit_value: limits.max_sequence_len,
                    actual: next_len,
                    position: Some(position(line_no)),
                });
            }
            self.sequence.push(uppercase);
        }

        Ok(())
    }

    /// Finalizes the section and emits the record-level diagnostics.
    pub fn finish(
        self,
        declared_length: u64,
        locus_line: u32,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<String, ImportError> {
        if self.sequence.is_empty() {
            return Err(ImportError::Malformed {
                code: "GB_SEQUENCE_EMPTY",
                message: "the ORIGIN section yields zero bases".to_string(),
                position: None,
            });
        }

        if self.normalized {
            diagnostics.push(Diagnostic::new(
                "GB_SEQUENCE_NORMALIZED",
                Severity::Info,
                "lowercase ORIGIN bases were uppercased",
            ));
        }

        let actual = self.sequence.len() as u64;
        if declared_length != actual {
            diagnostics.push(
                Diagnostic::new(
                    "GB_LOCUS_LENGTH_MISMATCH",
                    Severity::Warning,
                    format!(
                        "LOCUS declares {declared_length} bases but ORIGIN contains {actual}; the actual length wins"
                    ),
                )
                .with_position(position(locus_line)),
            );
        }

        Ok(self.sequence)
    }
}

fn position(line: u32) -> SourcePosition {
    SourcePosition::Genbank { line, column: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_numbering_and_uppercases() {
        let mut builder = OriginBuilder::new();
        builder
            .push_line("        1 atgc gtcg", 2, &Limits::default())
            .expect("line 1");
        builder
            .push_line("       61 tta", 3, &Limits::default())
            .expect("line 2");
        let sequence = builder.finish(11, 1, &mut Vec::new()).expect("finish");
        assert_eq!(sequence, "ATGCGTCGTTA");
    }

    #[test]
    fn rejects_u_and_invalid_characters() {
        let mut builder = OriginBuilder::new();
        assert_eq!(
            builder
                .push_line("        1 ACGTU", 2, &Limits::default())
                .expect_err("U is not DNA")
                .code(),
            "GB_RNA_BASE_IN_DNA"
        );
        let mut builder = OriginBuilder::new();
        assert_eq!(
            builder
                .push_line("        1 AC-GT", 2, &Limits::default())
                .expect_err("dash is not a base")
                .code(),
            "GB_ORIGIN_INVALID_CHAR"
        );
    }
}
