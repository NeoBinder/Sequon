//! GenBank LOCUS and header mapping.
//!
//! LOCUS supplies name, length, kind, molecule type, topology and the optional
//! division token. The header block is preserved as an ordered list of
//! key/value pairs for extras.genbank.header; DEFINITION also becomes the
//! canonical description.

use std::collections::HashSet;

use sequon_model::{Kind, MoleculeType, Topology};

use crate::api::ImportOptions;
use crate::error::{ImportError, RequiredField};
use crate::report::{Diagnostic, Severity, SourcePosition};

/// Parsed LOCUS line.
#[derive(Debug, Clone)]
pub(crate) struct Locus {
    /// Resolved record name.
    pub name: String,
    /// Declared sequence length, cross-checked against ORIGIN.
    pub declared_length: u64,
    /// Molecule kind; always Dna for a successful version 1 import.
    pub kind: Kind,
    /// Optional molecule subtype.
    pub molecule_type: Option<MoleculeType>,
    /// Linear or circular.
    pub topology: Topology,
    /// Optional LOCUS division token, restored by the M3 writer.
    pub division: Option<String>,
    /// 1-based LOCUS line.
    pub line: u32,
}

/// One ordered header field preserved in extras.genbank.header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HeaderEntry {
    /// Key exactly as written, without the trailing spaces.
    pub key: String,
    /// Value with continuation lines kept verbatim after a line feed.
    pub value: String,
}

/// Finalized header block.
#[derive(Debug, Clone, Default)]
pub(crate) struct Header {
    /// Ordered header entries.
    pub entries: Vec<HeaderEntry>,
    /// Normalized DEFINITION text, when present and non-empty.
    pub description: Option<String>,
}

/// Builds the header block line by line.
pub(crate) struct HeaderBuilder {
    entries: Vec<HeaderEntry>,
    unknown: HashSet<String>,
    bytes: u64,
    max_header_bytes: u64,
}

impl HeaderBuilder {
    /// Creates a builder for one record.
    pub fn new(max_header_bytes: u64) -> Self {
        Self {
            entries: Vec::new(),
            unknown: HashSet::new(),
            bytes: 0,
            max_header_bytes,
        }
    }

    /// Consumes one physical header line.
    pub fn push_line(
        &mut self,
        line: &str,
        line_no: u32,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<(), ImportError> {
        self.bytes = self.bytes.saturating_add(line.len() as u64 + 1);
        if self.bytes > self.max_header_bytes {
            return Err(ImportError::LimitExceeded {
                limit: "max_header_bytes",
                limit_value: self.max_header_bytes,
                actual: self.bytes,
                position: Some(position(line_no)),
            });
        }

        if line.trim().is_empty() {
            return Ok(());
        }

        let Some((key_start, value_start)) = header_entry_shape(line) else {
            if let Some(last) = self.entries.last_mut() {
                last.value.push('\n');
                last.value.push_str(line.trim_end());
            }
            return Ok(());
        };

        let key_end = line[key_start..]
            .find(char::is_whitespace)
            .map(|offset| key_start + offset)
            .unwrap_or(line.len());
        let key = line[key_start..key_end].trim().to_string();
        if key.is_empty() {
            return Ok(());
        }
        let value = line.get(value_start..).unwrap_or("").trim_end().to_string();

        if !is_known_key(&key) && self.unknown.insert(key.to_ascii_uppercase()) {
            diagnostics.push(
                Diagnostic::new(
                    "GB_HEADER_FIELD_UNKNOWN",
                    Severity::Info,
                    format!("header field '{key}' is not recognized; it was preserved"),
                )
                .with_position(position(line_no)),
            );
        }

        self.entries.push(HeaderEntry { key, value });
        Ok(())
    }

    /// Finalizes the block and derives description.
    pub fn finish(&mut self) -> Header {
        let description = self
            .entries
            .iter()
            .find(|entry| entry.key.eq_ignore_ascii_case("DEFINITION"))
            .map(|entry| entry.value.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|value| !value.is_empty());

        Header {
            entries: std::mem::take(&mut self.entries),
            description,
        }
    }
}

/// Parses the LOCUS line.
pub(crate) fn parse_locus(
    line: &str,
    line_no: u32,
    options: &ImportOptions,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Locus, ImportError> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.first() != Some(&"LOCUS") {
        return Err(malformed_locus(
            "LOCUS line does not start with LOCUS",
            line_no,
        ));
    }

    let mut length_index = None;
    for index in 1..tokens.len().saturating_sub(1) {
        if tokens[index]
            .chars()
            .all(|character| character.is_ascii_digit())
            && tokens[index + 1].eq_ignore_ascii_case("bp")
        {
            length_index = Some(index);
            break;
        }
    }
    let Some(length_index) = length_index else {
        return Err(malformed_locus(
            "LOCUS line has no numeric length followed by 'bp'",
            line_no,
        ));
    };
    let declared_length = tokens[length_index]
        .parse::<u64>()
        .map_err(|_| malformed_locus("LOCUS length is not a valid integer", line_no))?;

    let name = tokens[1..length_index].join(" ");
    let name = if name.is_empty() {
        options
            .name_hint
            .as_deref()
            .filter(|hint| !hint.trim().is_empty())
            .map(str::to_string)
            .ok_or_else(|| ImportError::MissingRequired {
                field: RequiredField::Name,
                position: Some(position(line_no)),
            })?
    } else {
        name
    };

    let molecule = tokens.get(length_index + 2).copied();
    let (kind, molecule_type) =
        classify_molecule(molecule, options.kind_hint, line_no, diagnostics)?;

    let topology_field = column_slice(line, 56, 63).trim();
    let topology = if !topology_field.is_empty() {
        if topology_field.eq_ignore_ascii_case("linear") {
            Topology::Linear
        } else if topology_field.eq_ignore_ascii_case("circular") {
            Topology::Circular
        } else {
            return Err(ImportError::Malformed {
                code: "RECORD_FIELD_INVALID",
                message: "LOCUS topology field is neither linear nor circular".to_string(),
                position: Some(position(line_no)),
            });
        }
    } else if let Some(found) = tokens.iter().skip(length_index + 3).find(|token| {
        token.eq_ignore_ascii_case("linear") || token.eq_ignore_ascii_case("circular")
    }) {
        if found.eq_ignore_ascii_case("linear") {
            Topology::Linear
        } else {
            Topology::Circular
        }
    } else if let Some(hint) = options.topology_hint {
        hint
    } else {
        return Err(ImportError::Malformed {
            code: "GB_TOPOLOGY_MISSING",
            message: "LOCUS line states no topology and no topology hint was supplied".to_string(),
            position: Some(position(line_no)),
        });
    };

    let division = parse_division(line, &tokens, length_index);

    Ok(Locus {
        name,
        declared_length,
        kind,
        molecule_type,
        topology,
        division,
        line: line_no,
    })
}

fn classify_molecule(
    token: Option<&str>,
    hint: Option<Kind>,
    line_no: u32,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(Kind, Option<MoleculeType>), ImportError> {
    let Some(token) = token else {
        return kind_from_hint(hint, line_no);
    };
    let upper = token.to_ascii_uppercase();
    match upper.as_str() {
        "DNA" | "DS-DNA" => Ok((Kind::Dna, Some(MoleculeType::DsDna))),
        "SS-DNA" => Ok((Kind::Dna, Some(MoleculeType::SsDna))),
        "PROTEIN" | "PRT" | "AA" => Err(unsupported_molecule(token, line_no)),
        _ if upper.ends_with("DNA") => {
            diagnostics.push(
                Diagnostic::new(
                    "MOLECULE_TYPE_UNKNOWN",
                    Severity::Warning,
                    format!("molecule subtype '{token}' is not known; stored as other"),
                )
                .with_position(position(line_no)),
            );
            Ok((Kind::Dna, Some(MoleculeType::Other)))
        }
        _ if upper.ends_with("RNA") => Err(unsupported_molecule(token, line_no)),
        _ => kind_from_hint(hint, line_no),
    }
}

fn kind_from_hint(
    hint: Option<Kind>,
    line_no: u32,
) -> Result<(Kind, Option<MoleculeType>), ImportError> {
    match hint {
        Some(Kind::Dna) => Ok((Kind::Dna, None)),
        Some(Kind::Rna | Kind::Protein) => Err(unsupported_molecule("molecule hint", line_no)),
        None => Err(ImportError::MissingRequired {
            field: RequiredField::Kind,
            position: Some(position(line_no)),
        }),
    }
}

fn parse_division(line: &str, tokens: &[&str], length_index: usize) -> Option<String> {
    let fixed = column_slice(line, 65, 67).trim();
    if is_division_token(fixed) {
        return Some(fixed.to_string());
    }
    tokens
        .iter()
        .skip(length_index + 3)
        .find(|token| is_division_token(token))
        .map(|token| (*token).to_string())
}

fn is_division_token(token: &str) -> bool {
    token.len() == 3
        && token
            .chars()
            .all(|character| character.is_ascii_uppercase())
}

fn header_entry_shape(line: &str) -> Option<(usize, usize)> {
    let first = first_non_space_byte(line)?;
    let key_start = if first == 0 {
        0
    } else if first == 2 {
        let token = line[2..].split_whitespace().next().unwrap_or("");
        if token.eq_ignore_ascii_case("ORGANISM") {
            2
        } else {
            return None;
        }
    } else {
        return None;
    };
    let key_end = line[key_start..]
        .find(char::is_whitespace)
        .map(|offset| key_start + offset)
        .unwrap_or(line.len());
    let value_start = if key_end < 12 {
        12
    } else {
        key_end.saturating_add(1)
    };
    Some((key_start, value_start.min(line.len())))
}

fn is_known_key(key: &str) -> bool {
    matches!(
        key.to_ascii_uppercase().as_str(),
        "DEFINITION"
            | "ACCESSION"
            | "VERSION"
            | "KEYWORDS"
            | "SOURCE"
            | "ORGANISM"
            | "REFERENCE"
            | "COMMENT"
    )
}

pub(crate) fn first_non_space_byte(line: &str) -> Option<usize> {
    line.char_indices()
        .find(|(_, character)| !character.is_whitespace())
        .map(|(index, _)| index)
}

fn column_slice(line: &str, start_col: u32, end_col: u32) -> &str {
    let start = char_to_byte(line, (start_col.saturating_sub(1)) as usize);
    let end = char_to_byte(line, end_col as usize);
    line.get(start..end).unwrap_or("")
}

fn char_to_byte(line: &str, index: usize) -> usize {
    line.char_indices()
        .nth(index)
        .map(|(byte, _)| byte)
        .unwrap_or(line.len())
}

fn malformed_locus(message: &str, line: u32) -> ImportError {
    ImportError::Malformed {
        code: "GB_MALFORMED_LOCUS",
        message: message.to_string(),
        position: Some(position(line)),
    }
}

fn unsupported_molecule(token: &str, line: u32) -> ImportError {
    ImportError::Unsupported {
        code: "GB_UNSUPPORTED_MOLECULE",
        message: format!("molecule type '{token}' is not supported; version 1 reads DNA only"),
        position: Some(position(line)),
    }
}

fn position(line: u32) -> SourcePosition {
    SourcePosition::Genbank { line, column: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_standard_locus_line() {
        let line =
            "LOCUS       SYNPLASMID               120 bp    DNA     circular SYN 01-JAN-2000";
        let mut diagnostics = Vec::new();
        let locus = parse_locus(line, 1, &ImportOptions::default(), &mut diagnostics)
            .expect("LOCUS must parse");
        assert_eq!(locus.name, "SYNPLASMID");
        assert_eq!(locus.declared_length, 120);
        assert_eq!(locus.kind, Kind::Dna);
        assert_eq!(locus.molecule_type, Some(MoleculeType::DsDna));
        assert_eq!(locus.topology, Topology::Circular);
        assert_eq!(locus.division.as_deref(), Some("SYN"));
    }

    #[test]
    fn missing_name_and_topology_are_typed_errors() {
        let mut diagnostics = Vec::new();
        let line = "LOCUS                       120 bp    DNA";
        let error = parse_locus(line, 1, &ImportOptions::default(), &mut diagnostics)
            .expect_err("name and topology are missing");
        assert_eq!(error.code(), "RECORD_REQUIRED_FIELD_MISSING");

        let line = "LOCUS       SYN161                   120 bp    DNA";
        let error = parse_locus(line, 1, &ImportOptions::default(), &mut diagnostics)
            .expect_err("topology is missing");
        assert_eq!(error.code(), "GB_TOPOLOGY_MISSING");
    }
}
