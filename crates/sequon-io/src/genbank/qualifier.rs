//! GenBank qualifier grammar.
//!
//! Values are kept as written: an unquoted value stays a string and records
//! GB_QUALIFIER_UNQUOTED, a quoted value loses only its outer quotes, and a
//! qualifier without '=' keeps a null value. Duplicate keys are the caller's
//! concern because qualifiers are stored as an ordered array.

use sequon_model::Qualifier;

use crate::error::ImportError;
use crate::limits::Limits;
use crate::report::{Diagnostic, Severity, SourcePosition};

use super::ParseFailure;

/// Parses one assembled qualifier, without its leading slash.
pub(crate) fn parse(
    text: &str,
    line: u32,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Qualifier, ParseFailure> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(malformed("qualifier has no key", line));
    }

    let Some(equals) = trimmed.find('=') else {
        return Ok(Qualifier::bare(trimmed));
    };

    let key = trimmed[..equals].trim();
    if key.is_empty() {
        return Err(malformed("qualifier has no key", line));
    }

    let raw = &trimmed[equals + 1..];
    let value = if raw.starts_with('"') {
        if raw.len() >= 2 && raw.ends_with('"') {
            raw[1..raw.len() - 1].to_string()
        } else {
            push_unquoted(diagnostics, line);
            raw.to_string()
        }
    } else {
        push_unquoted(diagnostics, line);
        raw.to_string()
    };

    let value_len = value.len() as u64;
    if value_len > limits.max_qualifier_value_bytes {
        return Err(ParseFailure::Fatal(ImportError::LimitExceeded {
            limit: "max_qualifier_value_bytes",
            limit_value: limits.max_qualifier_value_bytes,
            actual: value_len,
            position: Some(position(line)),
        }));
    }

    Ok(Qualifier {
        key: key.to_string(),
        value: Some(value),
    })
}

fn push_unquoted(diagnostics: &mut Vec<Diagnostic>, line: u32) {
    diagnostics.push(
        Diagnostic::new(
            "GB_QUALIFIER_UNQUOTED",
            Severity::Info,
            "qualifier value is unquoted; it was kept as written",
        )
        .with_position(position(line)),
    );
}

fn malformed(message: &str, line: u32) -> ParseFailure {
    ParseFailure::Record(ImportError::Malformed {
        code: "GB_FEATURES_MALFORMED",
        message: message.to_string(),
        position: Some(position(line)),
    })
}

fn position(line: u32) -> SourcePosition {
    SourcePosition::Genbank { line, column: None }
}
