//! Import-report and diagnostic types.
//!
//! The report is an event document: it is produced once per import and is never
//! written back into a record. Its shape is fixed by
//! schemas/import-report-1.schema.json, its byte rules by
//! docs/design/canonical-json-profile.md and its vocabulary by
//! docs/design/diagnostics.md.

use sequon_model::{DigestValue, Kind, Producer, SourceFormat};
use serde::{Deserialize, Serialize};

/// Schema identifier of a version 1 import report.
pub const REPORT_SCHEMA_ID: &str = "sequon/import-report/1";

/// Version of the diagnostic code table this crate emits.
pub const CODES_VERSION: &str = "2026-10-21/1";

/// Severity of one diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// The record is unusable.
    Error,
    /// The record is usable but something was lost or reinterpreted.
    Warning,
    /// Normal, but worth recording.
    Info,
}

/// Where in the source an event happened.
///
/// A report describes one source format, so every position in one report has the
/// same kind. GenBank positions carry a required line and an optional column;
/// SnapGene positions carry a required byte offset and optional packet fields.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SourcePosition {
    /// A 1-based line and optional 1-based column in GenBank text.
    Genbank {
        /// 1-based line number in the decoded source text.
        line: u32,
        /// 1-based column, present only when the adapter knows it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        column: Option<u32>,
    },
    /// A 0-based byte offset in a SnapGene container and optional packet fields.
    Snapgene {
        /// 0-based byte offset from the start of the file.
        offset: u64,
        /// TLV packet type, when the position belongs to a packet.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        packet_type: Option<u32>,
        /// 0-based index of the packet in the container's TLV sequence.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        packet_index: Option<u32>,
    },
}

/// One diagnostic event.
///
/// Nullable fields are always emitted, as JSON null when absent, so byte
/// determinism does not depend on optionality.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    /// Stable diagnostic code from docs/design/diagnostics.md.
    pub code: String,
    /// Severity fixed for that code.
    pub severity: Severity,
    /// One-line human-readable explanation; never contains sequence content.
    pub message: String,
    /// Where the event happened; null when it has no source position.
    #[serde(default)]
    pub position: Option<SourcePosition>,
    /// Record the event belongs to; null when it applies to the whole input.
    #[serde(default)]
    pub record_index: Option<u32>,
}

impl Diagnostic {
    /// Creates a diagnostic with no position and no record index.
    pub fn new(code: impl Into<String>, severity: Severity, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            severity,
            message: message.into(),
            position: None,
            record_index: None,
        }
    }

    /// Attaches a source position.
    pub fn with_position(mut self, position: SourcePosition) -> Self {
        self.position = Some(position);
        self
    }

    /// Attaches a record index.
    pub fn with_record_index(mut self, record_index: u32) -> Self {
        self.record_index = Some(record_index);
        self
    }
}

/// Summary of one returned record in a report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordSummary {
    /// 0-based parse-order ordinal of the record.
    pub index: u32,
    /// Record name as it appears in the canonical record.
    pub name: String,
    /// Molecule kind.
    pub kind: Kind,
    /// Number of bases or residues.
    pub length: u64,
    /// Copy of the record's direction-sensitive sequence digest.
    pub sequence_digest: DigestValue,
    /// Copy of the record's canonical digest.
    pub canonical_digest: DigestValue,
}

/// One skipped block or feature, kept as a fingerprint rather than as bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dropped {
    /// Adapter-defined kind of the skipped item.
    pub what: String,
    /// Machine-readable reason from the closed vocabulary in the diagnostics note.
    pub reason: String,
    /// Record the item belonged to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_index: Option<u32>,
    /// Where the item was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<SourcePosition>,
    /// Convenience copy of the packet type for SnapGene items.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packet_type: Option<u32>,
    /// Convenience copy of the 0-based byte offset of the skipped bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
    /// Length of the skipped bytes, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<u64>,
    /// Digest of the skipped bytes, so the loss is auditable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<DigestValue>,
    /// GenBank feature key of a skipped feature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_key: Option<String>,
    /// Source location text of a skipped feature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location_text: Option<String>,
}

/// Encoding of a preserved block. Version 1 defines base64 only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PreservedEncoding {
    /// Base64 text of the raw bytes.
    Base64,
}

/// One raw source block kept verbatim in the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preserved {
    /// Adapter-defined kind of the kept block.
    pub what: String,
    /// Payload encoding; base64 in version 1.
    pub encoding: PreservedEncoding,
    /// The raw bytes, base64-encoded.
    pub data: String,
    /// Convenience copy of the packet type for SnapGene blocks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packet_type: Option<u32>,
    /// Convenience copy of the 0-based byte offset of the kept bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
    /// Length of the kept bytes before base64 encoding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<u64>,
    /// Digest of the kept bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<DigestValue>,
    /// Record the block belonged to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_index: Option<u32>,
}

/// The import report returned together with the import outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportReport {
    /// Schema identifier; REPORT_SCHEMA_ID for a valid version 1 report.
    pub schema: String,
    /// Format of the imported source.
    pub source_format: SourceFormat,
    /// Digest of the exact source bytes.
    pub source_digest: DigestValue,
    /// Length of the source input in bytes.
    pub source_bytes: u64,
    /// Producer that parsed the source.
    pub parser: Producer,
    /// SnapGene export version; always emitted, null for GenBank.
    #[serde(default)]
    pub export_version: Option<u32>,
    /// Version of the diagnostic code table used by this report.
    pub codes_version: String,
    /// One entry per returned record, in output order.
    pub records: Vec<RecordSummary>,
    /// Diagnostics, sorted deterministically before serialization.
    pub diagnostics: Vec<Diagnostic>,
    /// Skipped blocks or features, with fingerprints.
    pub dropped: Vec<Dropped>,
    /// Raw blocks kept verbatim.
    pub preserved: Vec<Preserved>,
}

impl ImportReport {
    /// Creates an empty report for one source.
    pub fn new(
        source_format: SourceFormat,
        source_digest: DigestValue,
        source_bytes: u64,
        parser: Producer,
        export_version: Option<u32>,
    ) -> Self {
        Self {
            schema: REPORT_SCHEMA_ID.to_string(),
            source_format,
            source_digest,
            source_bytes,
            parser,
            export_version,
            codes_version: CODES_VERSION.to_string(),
            records: Vec::new(),
            diagnostics: Vec::new(),
            dropped: Vec::new(),
            preserved: Vec::new(),
        }
    }

    /// Sorts diagnostics by the order fixed in docs/design/diagnostics.md.
    ///
    /// Positioned events come first in ascending source order, then code, and
    /// the sort is stable so equal keys keep emission order. Two imports of the
    /// same bytes therefore produce the same order.
    pub fn sort_diagnostics(&mut self) {
        self.diagnostics
            .sort_by(|left, right| diagnostic_key(left).cmp(&diagnostic_key(right)));
    }
}

fn diagnostic_key(diagnostic: &Diagnostic) -> (u8, u64, u64, u64, &str) {
    match &diagnostic.position {
        None => (1, 0, 0, 0, diagnostic.code.as_str()),
        Some(SourcePosition::Genbank { line, column }) => (
            0,
            u64::from(*line),
            u64::from(column.unwrap_or(0)),
            0,
            diagnostic.code.as_str(),
        ),
        Some(SourcePosition::Snapgene {
            offset,
            packet_type,
            packet_index,
        }) => (
            0,
            *offset,
            u64::from(packet_index.unwrap_or(0)),
            u64::from(packet_type.unwrap_or(0)),
            diagnostic.code.as_str(),
        ),
    }
}
