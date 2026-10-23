//! GenBank adapter.
//!
//! The accepted mapping, diagnostics and writer rules are
//! docs/design/genbank-adapter.md; the record contract is
//! docs/design/canonical-model.md and the code vocabulary is
//! docs/design/diagnostics.md.
//!
//! M1 implements the reader. The writer entry point is a placeholder until M3.

use sequon_model::Record;

use crate::api::{ImportOptions, ImportResult, WriteError, WriteOptions, WriteOutcome};
use crate::error::ImportError;

mod features;
mod header;
mod location;
mod origin;
mod qualifier;
mod reader;
mod writer;

/// State shared by the record parsers.
pub(crate) struct ParseContext<'a> {
    /// The configured limits, shared by the line parsers.
    pub limits: &'a crate::limits::Limits,
    /// Set once GB_FEATURE_COLUMN_MISALIGNED has been emitted for this input.
    pub column_misaligned_emitted: &'a mut bool,
}

/// Why one record could not be turned into a canonical record.
///
/// A limit always aborts the import, because LIMIT_EXCEEDED is a typed error
/// and never a report diagnostic. Every other record-level failure is reported
/// as an error diagnostic when another record is usable, and as a typed error
/// only when no usable record remains.
pub(crate) enum ParseFailure {
    /// The record is unusable, but the import can continue with other records.
    Record(ImportError),
    /// A configured limit was exceeded; the import stops.
    Fatal(ImportError),
}

/// Imports one GenBank input.
pub fn import(bytes: &[u8], options: &ImportOptions) -> ImportResult {
    reader::import(bytes, options)
}

/// Writes one record as GenBank text.
pub fn write_genbank(record: &Record, options: &WriteOptions) -> Result<WriteOutcome, WriteError> {
    writer::write(record, options)
}

pub(crate) fn line_number(index: usize) -> u32 {
    u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX)
}
