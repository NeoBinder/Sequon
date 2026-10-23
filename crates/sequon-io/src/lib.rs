//! Format adapters for Sequon.
//!
//! Each supported file format gets one adapter that parses into the canonical
//! model and, where the model allows it losslessly, serializes back out.
//! Adapters accept untrusted input and must report problems instead of
//! panicking or dropping data silently.
//!
//! This crate also owns the shared import vocabulary: the configurable limits,
//! the typed ImportError, the ImportReport event document, content detection and
//! the canonical JSON module. Those types are format-independent, so the crate
//! compiles with --no-default-features; adapters are added behind features.
//!
//! The canonical model itself is owned by the sequon-model crate and its
//! semantics are fixed by docs/design/canonical-model.md. Byte-level output
//! rules live in docs/design/canonical-json-profile.md and the diagnostic
//! vocabulary in docs/design/diagnostics.md.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod api;
pub mod detect;
pub mod error;
pub mod json;
pub mod limits;
pub mod report;
mod schema;

#[cfg(any(feature = "genbank", feature = "snapgene"))]
pub(crate) mod so;

#[cfg(feature = "genbank")]
pub mod genbank;
#[cfg(feature = "snapgene")]
pub mod snapgene;

pub use api::{
    Format, ImportOptions, ImportOutcome, ImportResult, MultiplePolicy, WriteError, WriteOptions,
    WriteOutcome,
};
pub use error::{ImportError, RequiredField};
pub use limits::Limits;
pub use report::{
    Diagnostic, Dropped, ImportReport, Preserved, PreservedEncoding, RecordSummary, Severity,
    SourcePosition, CODES_VERSION, REPORT_SCHEMA_ID,
};
pub use schema::{REPORT_SCHEMA, REPORT_SCHEMA_URN};

/// Detects the format of an input buffer.
pub fn detect(bytes: &[u8], limits: &Limits) -> Result<Format, ImportError> {
    detect::detect(bytes, limits)
}

/// Imports an input using the requested format or content detection.
pub fn import(bytes: &[u8], options: &ImportOptions) -> ImportResult {
    detect::import(bytes, options)
}

/// Imports a GenBank input.
#[cfg(feature = "genbank")]
pub fn import_genbank(bytes: &[u8], options: &ImportOptions) -> ImportResult {
    genbank::import(bytes, options)
}

/// Imports a SnapGene .dna input.
#[cfg(feature = "snapgene")]
pub fn import_snapgene(bytes: &[u8], options: &ImportOptions) -> ImportResult {
    snapgene::import(bytes, options)
}

/// Writes one record as GenBank text.
#[cfg(feature = "genbank")]
pub fn write_genbank(
    record: &sequon_model::Record,
    options: &WriteOptions,
) -> Result<WriteOutcome, WriteError> {
    genbank::write_genbank(record, options)
}
