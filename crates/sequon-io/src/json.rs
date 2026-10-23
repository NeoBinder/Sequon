//! Canonical JSON for records.
//!
//! The byte rules are fixed by docs/design/canonical-json-profile.md and are
//! implemented here in one place:
//!
//! - UTF-8 without a byte order mark, LF line ends, two-space indentation and
//!   exactly one trailing newline;
//! - field order is the struct declaration order of sequon-model, which is the
//!   schema declaration order;
//! - nullable optional fields are always emitted, as JSON null when absent;
//! - extras object keys are sorted because the payload is a BTreeMap;
//! - there are no floating-point numbers anywhere, and a document that contains
//!   one is rejected on parse;
//! - canonical structs deny unknown fields, so a typo fails instead of being
//!   silently ignored.
//!
//! Parsing performs structural validation only. Call Record::validate() for the
//! cross-field invariants and sequon_algo::verify_digests() to check that the
//! stored digests match the sequence.

use std::error::Error;
use std::fmt;

use sequon_model::Record;

use crate::report::ImportReport;

/// Error returned by the canonical JSON module.
#[derive(Debug)]
pub enum JsonError {
    /// Serializing a record to JSON failed.
    Serialize(serde_json::Error),
    /// Parsing JSON into a record failed.
    Parse(serde_json::Error),
}

impl fmt::Display for JsonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsonError::Serialize(error) => write!(formatter, "cannot serialize record: {error}"),
            JsonError::Parse(error) => write!(formatter, "cannot parse record: {error}"),
        }
    }
}

impl Error for JsonError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            JsonError::Serialize(error) | JsonError::Parse(error) => Some(error),
        }
    }
}

/// Serializes a record to canonical JSON bytes.
///
/// The output is pretty-printed with two spaces, uses LF line ends and ends with
/// exactly one newline. Field order, nullable-field emission and extras key order
/// come from the sequon-model types; this function does not reorder anything.
///
/// The record is not validated here: call Record::validate() and
/// sequon_algo::verify_digests() first when the record came from an untrusted
/// source, so an invalid record is never persisted as canonical JSON.
pub fn to_canonical_json_bytes(record: &Record) -> Result<Vec<u8>, JsonError> {
    let mut bytes = serde_json::to_vec_pretty(record).map_err(JsonError::Serialize)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Parses canonical JSON bytes into a record.
///
/// Unknown fields are rejected, nullable fields may be absent or null, and a
/// floating-point number anywhere (including inside extras) is rejected with an
/// error that names the canonical JSON rule. Parsing does not run
/// Record::validate(); call it before using the record.
pub fn parse_canonical_json(bytes: &[u8]) -> Result<Record, JsonError> {
    serde_json::from_slice(bytes).map_err(JsonError::Parse)
}

/// Serializes an import report to canonical JSON bytes, with the same byte rules
/// as a record.
pub fn to_canonical_report_json_bytes(report: &ImportReport) -> Result<Vec<u8>, JsonError> {
    let mut bytes = serde_json::to_vec_pretty(report).map_err(JsonError::Serialize)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Parses canonical JSON bytes into an import report.
///
/// Unknown fields are rejected and floating-point numbers are impossible by the
/// report types. Parsing checks structure only; the adapter that produced the
/// report is responsible for the diagnostics invariants in its design note.
pub fn parse_import_report(bytes: &[u8]) -> Result<ImportReport, JsonError> {
    serde_json::from_slice(bytes).map_err(JsonError::Parse)
}
