//! Where a record came from.

use serde::{Deserialize, Serialize};

use crate::digest::DigestValue;

/// How a record was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProducerKind {
    /// A format adapter parsed source bytes.
    Parser,
    /// An algorithm derived the record.
    Algorithm,
    /// A person authored or edited the record.
    Manual,
}

/// Format a record was imported from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SourceFormat {
    /// GenBank flat file.
    #[serde(rename = "genbank")]
    Genbank,
    /// SnapGene .dna container.
    #[serde(rename = "snapgene-dna")]
    SnapgeneDna,
}

/// The tool that produced a record.
///
/// The field order is the canonical JSON field order: kind, name, version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Producer {
    /// Who produced the record.
    pub kind: ProducerKind,
    /// Producer name, for example sequon-io; never empty in a valid record.
    pub name: String,
    /// Producer version, for example 0.1.0; never empty in a valid record.
    pub version: String,
}

/// Where a record came from and how its source bytes can be re-read.
///
/// Warnings, dropped data and preserved blocks never live here; they belong to
/// the import report (D6/D7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    /// Producer of the record.
    pub producer: Producer,
    /// Format the record was imported from.
    pub source_format: SourceFormat,
    /// SHA-256 of the exact source bytes.
    pub source_digest: DigestValue,
    /// Length of the source input in bytes.
    pub source_bytes: u64,
    /// SnapGene export version, or null for formats that do not carry one.
    #[serde(default)]
    pub export_version: Option<u32>,
}
