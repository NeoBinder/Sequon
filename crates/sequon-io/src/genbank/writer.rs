//! GenBank writer.
//!
//! The accepted mapping and layout rules are docs/design/genbank-adapter.md
//! section 9. The writer accepts a validated canonical record and produces
//! deterministic GenBank text. Non-fatal writer diagnostics travel in
//! WriteOutcome; failures that prevent producing bytes are WriteError values.

use sequon_model::{
    Feature, JsonValue, Kind, Location, MoleculeType, Qualifier, Record, Segment, Topology,
    OPERATOR_JOIN, OPERATOR_ORDER, OPERATOR_SPAN,
};

use crate::api::{WriteError, WriteOptions, WriteOutcome};
use crate::report::{Diagnostic, Severity};

const DEFAULT_DATE: &str = "01-JAN-2000";
const MONTHS: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];
const NAME_WIDTH: usize = 16;
const QUALIFIER_INDENT: usize = 21;

/// Writes one canonical record as GenBank text.
pub(crate) fn write(record: &Record, options: &WriteOptions) -> Result<WriteOutcome, WriteError> {
    if record.kind != Kind::Dna {
        return Err(WriteError::UnsupportedKind { kind: record.kind });
    }
    record
        .validate()
        .map_err(|error| invalid(format!("record validation failed: {error}")))?;

    let mut diagnostics = Vec::new();
    let date = resolve_date(options.date.as_deref())?;
    let extras = read_extras(record, options.include_header_extras)?;
    let molecule_token = molecule_token(record.molecule_type, &mut diagnostics)?;
    let name = truncate_name(&record.name, &mut diagnostics);
    let length = record.sequence.len();

    let mut output = String::new();
    write_locus(
        &mut output,
        &name,
        length,
        molecule_token,
        record.topology,
        extras.division.as_deref(),
        &date,
    )?;
    write_header(&mut output, record, &extras, options)?;
    write_features(&mut output, record, options.line_width, &mut diagnostics)?;
    write_origin(&mut output, &record.sequence)?;
    output.push_str("//\n");

    diagnostics.sort_by(|left, right| left.code.cmp(&right.code));
    Ok(WriteOutcome {
        bytes: output.into_bytes(),
        diagnostics,
    })
}

struct GenbankExtras {
    division: Option<String>,
    header: Vec<(String, String)>,
}

fn read_extras(record: &Record, include_header: bool) -> Result<GenbankExtras, WriteError> {
    let Some(payload) = record.extras.get("genbank") else {
        return Ok(GenbankExtras {
            division: None,
            header: Vec::new(),
        });
    };
    let Some(object) = payload.as_object() else {
        return Err(invalid(
            "extras.genbank must be an object when the GenBank adapter owns it",
        ));
    };

    let division = match object.get("division") {
        None => None,
        Some(JsonValue::String(value)) => {
            if !is_division(value) {
                return Err(invalid(
                    "extras.genbank.division must be exactly three uppercase ASCII letters",
                ));
            }
            Some(value.clone())
        }
        Some(_) => {
            return Err(invalid("extras.genbank.division must be a string"));
        }
    };

    let mut header = Vec::new();
    if include_header {
        match object.get("header") {
            None => {}
            Some(JsonValue::Array(entries)) => {
                for entry in entries {
                    let Some(entry_object) = entry.as_object() else {
                        return Err(invalid(
                            "extras.genbank.header entries must be {key, value} objects",
                        ));
                    };
                    let Some(key) = entry_object.get("key").and_then(JsonValue::as_str) else {
                        return Err(invalid("extras.genbank.header entry key must be a string"));
                    };
                    let Some(value) = entry_object.get("value").and_then(JsonValue::as_str) else {
                        return Err(invalid(
                            "extras.genbank.header entry value must be a string",
                        ));
                    };
                    validate_header_key(key)?;
                    if value.contains('\r') {
                        return Err(invalid("header values must not contain a carriage return"));
                    }
                    header.push((key.to_string(), value.to_string()));
                }
            }
            Some(_) => {
                return Err(invalid("extras.genbank.header must be an array"));
            }
        }
    }

    Ok(GenbankExtras { division, header })
}

fn resolve_date(date: Option<&str>) -> Result<String, WriteError> {
    let Some(date) = date else {
        return Ok(DEFAULT_DATE.to_string());
    };
    if is_date(date) {
        Ok(date.to_string())
    } else {
        Err(invalid(
            "WriteOptions.date must be DD-MON-YYYY with an uppercase month",
        ))
    }
}

fn is_date(date: &str) -> bool {
    if !date.is_ascii() || date.len() != 11 {
        return false;
    }
    let bytes = date.as_bytes();
    if !bytes[0].is_ascii_digit()
        || !bytes[1].is_ascii_digit()
        || bytes[2] != b'-'
        || bytes[6] != b'-'
        || !bytes[7..11].iter().all(u8::is_ascii_digit)
    {
        return false;
    }
    let month = &date[3..6];
    if !MONTHS.contains(&month) {
        return false;
    }
    let day = date[..2].parse::<u8>().unwrap_or(0);
    (1..=31).contains(&day)
}

fn is_division(value: &str) -> bool {
    value.len() == 3 && value.bytes().all(|byte| byte.is_ascii_uppercase())
}

fn molecule_token(
    molecule_type: Option<MoleculeType>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<&'static str, WriteError> {
    match molecule_type {
        None => {
            diagnostics.push(Diagnostic::new(
                "WRITE_MOLECULE_TYPE_DERIVED",
                Severity::Info,
                "molecule_type was absent; DNA was derived from kind dna",
            ));
            Ok("DNA")
        }
        Some(MoleculeType::DsDna) => Ok("DNA"),
        Some(MoleculeType::SsDna) => Ok("ss-DNA"),
        Some(MoleculeType::Other) => Err(WriteError::UnsupportedFeature {
            message:
                "WRITE_UNSUPPORTED_MOLECULE_TYPE: molecule_type other has no legal LOCUS token"
                    .to_string(),
        }),
    }
}

fn truncate_name(name: &str, diagnostics: &mut Vec<Diagnostic>) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= NAME_WIDTH {
        return name.to_string();
    }
    diagnostics.push(Diagnostic::new(
        "WRITE_NAME_TRUNCATED",
        Severity::Warning,
        format!(
            "record name has {} characters and was truncated to {NAME_WIDTH}",
            chars.len()
        ),
    ));
    chars.into_iter().take(NAME_WIDTH).collect()
}

fn write_locus(
    output: &mut String,
    name: &str,
    length: usize,
    molecule: &str,
    topology: Topology,
    division: Option<&str>,
    date: &str,
) -> Result<(), WriteError> {
    let length = length.to_string();
    if length.len() > 11 {
        return Err(invalid(
            "sequence length does not fit the LOCUS length field",
        ));
    }
    let topology = match topology {
        Topology::Linear => "linear",
        Topology::Circular => "circular",
    };

    output.push_str("LOCUS");
    output.push_str("       ");
    push_padded(output, name, NAME_WIDTH, false);
    output.push(' ');
    push_padded(output, &length, 11, true);
    output.push_str(" bp");
    output.push_str("    ");
    push_padded(output, molecule, 6, false);
    output.push_str("  ");
    push_padded(output, topology, 8, true);
    output.push(' ');
    match division {
        Some(division) => output.push_str(division),
        None => output.push_str("   "),
    }
    output.push(' ');
    output.push_str(date);
    output.push('\n');
    Ok(())
}

fn push_padded(output: &mut String, value: &str, width: usize, right: bool) {
    let count = value.chars().count();
    let padding = width.saturating_sub(count);
    if right {
        output.extend(std::iter::repeat_n(' ', padding));
        output.push_str(value);
    } else {
        output.push_str(value);
        output.extend(std::iter::repeat_n(' ', padding));
    }
}

fn write_header(
    output: &mut String,
    record: &Record,
    extras: &GenbankExtras,
    options: &WriteOptions,
) -> Result<(), WriteError> {
    if options.include_header_extras {
        let has_definition = extras
            .header
            .iter()
            .any(|(key, _)| key.eq_ignore_ascii_case("DEFINITION"));
        if !has_definition {
            if let Some(description) = &record.description {
                write_header_entry(output, "DEFINITION", description, false)?;
            }
        }
        for (key, value) in &extras.header {
            if is_structural_header_key(key) {
                continue;
            }
            write_header_entry(output, key, value, key.eq_ignore_ascii_case("ORGANISM"))?;
        }
    } else if let Some(description) = &record.description {
        write_header_entry(output, "DEFINITION", description, false)?;
    }
    Ok(())
}

fn is_structural_header_key(key: &str) -> bool {
    key.eq_ignore_ascii_case("LOCUS")
        || key.eq_ignore_ascii_case("FEATURES")
        || key.eq_ignore_ascii_case("ORIGIN")
}

fn validate_header_key(key: &str) -> Result<(), WriteError> {
    if key.is_empty()
        || key
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(invalid("header keys must be non-empty tokens"));
    }
    Ok(())
}

fn write_header_entry(
    output: &mut String,
    key: &str,
    value: &str,
    indented: bool,
) -> Result<(), WriteError> {
    if value.contains('\r') {
        return Err(invalid("header values must not contain a carriage return"));
    }
    let mut prefix = if indented {
        format!("  {key}")
    } else {
        key.to_string()
    };
    let prefix_len = prefix.chars().count();
    if prefix_len < 12 {
        prefix.extend(std::iter::repeat_n(' ', 12 - prefix_len));
    } else {
        prefix.push(' ');
    }

    let mut lines = value.split('\n');
    let first = lines.next().unwrap_or("");
    prefix.push_str(first.trim_end());
    let line = prefix.trim_end();
    if !line.is_empty() {
        output.push_str(line);
    }
    output.push('\n');

    for continuation in lines {
        let continuation = continuation.trim_end();
        if continuation.is_empty() {
            output.push('\n');
        } else if continuation.chars().next().is_some_and(char::is_whitespace) {
            output.push_str(continuation);
            output.push('\n');
        } else {
            output.push_str("            ");
            output.push_str(continuation);
            output.push('\n');
        }
    }
    Ok(())
}

fn write_features(
    output: &mut String,
    record: &Record,
    line_width: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(), WriteError> {
    output.push_str("FEATURES             Location/Qualifiers\n");
    for feature in &record.features {
        write_feature(
            output,
            feature,
            record.length,
            record.topology,
            line_width,
            diagnostics,
        )?;
    }
    Ok(())
}

fn write_feature(
    output: &mut String,
    feature: &Feature,
    length: u64,
    topology: Topology,
    line_width: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(), WriteError> {
    validate_feature_key(&feature.key)?;
    let location = serialize_location(&feature.location, length, topology, diagnostics)?;

    output.push_str("     ");
    let key_len = feature.key.chars().count();
    if key_len <= 15 {
        push_padded(output, &feature.key, 15, false);
        output.push(' ');
    } else {
        output.push_str(&feature.key);
        output.push(' ');
    }
    output.push_str(&location);
    output.push('\n');

    for qualifier in &feature.qualifiers {
        write_qualifier(output, qualifier, line_width, diagnostics)?;
    }
    Ok(())
}

fn validate_feature_key(key: &str) -> Result<(), WriteError> {
    if key.is_empty()
        || key
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(invalid("feature keys must be non-empty tokens"));
    }
    Ok(())
}

fn write_qualifier(
    output: &mut String,
    qualifier: &Qualifier,
    line_width: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(), WriteError> {
    validate_qualifier_key(&qualifier.key)?;
    let indent = " ".repeat(QUALIFIER_INDENT);
    match &qualifier.value {
        None => {
            output.push_str(&indent);
            output.push('/');
            output.push_str(&qualifier.key);
            output.push('\n');
        }
        Some(value) => {
            if value.contains('\n') || value.contains('\r') {
                return Err(invalid(
                    "qualifier values must not contain line breaks; GenBank has no lossless spelling for them",
                ));
            }
            let normalized = if value.contains('"') {
                diagnostics.push(Diagnostic::new(
                    "WRITE_QUOTE_NORMALIZED",
                    Severity::Info,
                    "a qualifier value contained a double quote; it was replaced with two apostrophes",
                ));
                value.replace('"', "''")
            } else {
                value.clone()
            };
            let prefix = format!("{indent}/{}=\"", qualifier.key);
            for line in wrap_qualifier(&prefix, &normalized, line_width) {
                output.push_str(&line);
                output.push('\n');
            }
        }
    }
    Ok(())
}

fn validate_qualifier_key(key: &str) -> Result<(), WriteError> {
    if key.is_empty()
        || key.chars().any(|character| {
            character.is_whitespace()
                || character.is_control()
                || matches!(character, '/' | '=' | '"')
        })
    {
        return Err(invalid("qualifier keys must be non-empty tokens"));
    }
    Ok(())
}

fn wrap_qualifier(prefix: &str, value: &str, line_width: usize) -> Vec<String> {
    let suffix = "\"";
    if value.is_empty() {
        return vec![format!("{prefix}{suffix}")];
    }
    let indent = " ".repeat(QUALIFIER_INDENT);
    let mut lines = Vec::new();
    let mut line = prefix.to_string();
    let mut remaining = value;

    loop {
        if line.len() + remaining.len() + suffix.len() <= line_width {
            line.push_str(remaining);
            line.push_str(suffix);
            lines.push(line);
            break;
        }

        let available = line_width.saturating_sub(line.len());
        let mut limit = available.min(remaining.len());
        while limit > 0 && !remaining.is_char_boundary(limit) {
            limit -= 1;
        }
        if let Some(position) = remaining[..limit].rfind(' ') {
            if position > 0 {
                line.push_str(&remaining[..position]);
                lines.push(line);
                remaining = &remaining[position + 1..];
                line = indent.clone();
                continue;
            }
        }

        if let Some(position) = remaining.find(' ') {
            if position == 0 {
                line.push(' ');
                remaining = &remaining[1..];
            } else {
                line.push_str(&remaining[..position]);
                lines.push(line);
                remaining = &remaining[position + 1..];
                line = indent.clone();
            }
            continue;
        }

        line.push_str(remaining);
        line.push_str(suffix);
        lines.push(line);
        break;
    }
    lines
}

fn serialize_location(
    location: &Location,
    length: u64,
    topology: Topology,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<String, WriteError> {
    let operator = match location.operator.as_str() {
        OPERATOR_SPAN => {
            if location.segments.len() != 1 {
                return Err(invalid("span locations must carry exactly one segment"));
            }
            None
        }
        OPERATOR_JOIN => Some(OPERATOR_JOIN),
        OPERATOR_ORDER => Some(OPERATOR_ORDER),
        _ => return Err(invalid("unknown location operator")),
    };
    if location.segments.is_empty() {
        return Err(invalid("locations must carry at least one segment"));
    }

    let mut parts = Vec::with_capacity(location.segments.len());
    for segment in &location.segments {
        parts.push(serialize_segment(segment, length, topology)?);
    }
    let body = match operator {
        Some(operator) => format!("{operator}({})", parts.join(",")),
        None => parts.join(","),
    };

    match location.strand {
        -1 => Ok(format!("complement({body})")),
        0 => {
            diagnostics.push(Diagnostic::new(
                "WRITE_STRAND_LOST",
                Severity::Warning,
                "a location with strand 0 was written as a plain interval; a re-import reads strand 1",
            ));
            Ok(body)
        }
        1 => Ok(body),
        _ => Err(invalid("location strand must be -1, 0 or 1")),
    }
}

fn serialize_segment(
    segment: &Segment,
    length: u64,
    topology: Topology,
) -> Result<String, WriteError> {
    if segment.start == segment.end {
        if segment.start_partial || segment.end_partial {
            return Err(invalid(
                "a between-positions segment cannot carry a partial marker",
            ));
        }
        if segment.start == 0 {
            return Err(invalid("between-positions sites are 1-based"));
        }
        let site = segment.start;
        match topology {
            Topology::Linear if site >= length => {
                return Err(invalid(
                    "a linear between-positions site must be before the last base",
                ))
            }
            Topology::Circular if site > length => {
                return Err(invalid(
                    "a circular between-positions site must be at or before the last base",
                ))
            }
            _ => {}
        }
        let right = site
            .checked_add(1)
            .ok_or_else(|| invalid("between-positions coordinate overflow"))?;
        return Ok(format!("{site}^{right}"));
    }

    if segment.start > segment.end {
        return Err(invalid("segment start must not be after its end"));
    }
    // A linear record keeps every segment inside the sequence. A circular
    // record may carry one unwrapped origin-crossing span: the canonical
    // payload stores `end` past `length` (at most one wrap) and the writer
    // prints that single range, never a split join.
    match topology {
        Topology::Linear => {
            if segment.end > length {
                return Err(invalid("segment end exceeds the sequence length"));
            }
        }
        Topology::Circular => {
            if segment.start >= length {
                return Err(invalid(
                    "a circular segment must start before the sequence end",
                ));
            }
            let max_end = segment
                .start
                .checked_add(length)
                .ok_or_else(|| invalid("circular segment coordinate overflow"))?;
            if segment.end > max_end {
                return Err(invalid(
                    "a circular segment must not wrap more than once around the origin",
                ));
            }
        }
    }

    let start = segment.start + 1;
    if segment.end == start {
        return Ok(if segment.start_partial && segment.end_partial {
            format!("<{start}..>{start}")
        } else if segment.start_partial {
            format!("<{start}")
        } else if segment.end_partial {
            format!(">{start}")
        } else {
            start.to_string()
        });
    }

    let mut text = String::new();
    if segment.start_partial {
        text.push('<');
    }
    text.push_str(&start.to_string());
    text.push_str("..");
    if segment.end_partial {
        text.push('>');
    }
    text.push_str(&segment.end.to_string());
    Ok(text)
}

fn write_origin(output: &mut String, sequence: &str) -> Result<(), WriteError> {
    if sequence.is_empty() {
        return Err(invalid("an empty sequence cannot be written"));
    }
    output.push_str("ORIGIN\n");
    for (index, chunk) in sequence.as_bytes().chunks(60).enumerate() {
        let mut groups = Vec::with_capacity(chunk.len() / 10 + 1);
        for group in chunk.chunks(10) {
            let text = std::str::from_utf8(group)
                .map_err(|_| invalid("sequence contains non-UTF-8 bytes"))?;
            groups.push(text);
        }
        let number = index * 60 + 1;
        output.push_str(&format!("{number:>9} {}\n", groups.join(" ")));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> WriteError {
    WriteError::InvalidRecord {
        message: message.into(),
    }
}
