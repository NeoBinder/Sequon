//! The record envelope and its value-level enums.
//!
//! Field order in this file is the canonical JSON field order: schema, kind,
//! molecule_type, topology, name, description, length, sequence, digests,
//! features, provenance, extras. Do not reorder the fields.

use serde::{Deserialize, Serialize};

use crate::digest::Digests;
use crate::extras::Extras;
use crate::feature::Feature;
use crate::provenance::Provenance;

/// Schema identifier of a version 1 canonical record.
pub const SCHEMA_ID: &str = "sequon/record/1";

/// Molecule kind.
///
/// Version 1 adapters only produce Kind::Dna; the other variants exist so a
/// later milestone does not need a new schema version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Deoxyribonucleic acid.
    Dna,
    /// Ribonucleic acid. Reserved; version 1 adapters reject it.
    Rna,
    /// Protein. Reserved; version 1 adapters reject it.
    Protein,
}

/// Sequence topology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Topology {
    /// A linear sequence.
    Linear,
    /// A circular sequence.
    Circular,
}

/// Optional molecule type as stated by the source format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MoleculeType {
    /// Double-stranded DNA.
    #[serde(rename = "ds-DNA")]
    DsDna,
    /// Single-stranded DNA.
    #[serde(rename = "ss-DNA")]
    SsDna,
    /// A DNA molecule whose subtype is outside the named values.
    #[serde(rename = "other")]
    Other,
}

/// One canonical sequence record.
///
/// The record is immutable by contract: editing produces a new record. A record
/// is structurally valid after deserialization, but it is only semantically
/// valid after Record::validate() succeeds, because JSON Schema cannot compare
/// fields with each other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    /// Schema identifier; SCHEMA_ID for a valid version 1 record.
    pub schema: String,
    /// Molecule kind.
    pub kind: Kind,
    /// Optional, nullable molecule type. Always emitted; null when unknown.
    #[serde(default)]
    pub molecule_type: Option<MoleculeType>,
    /// Linear or circular.
    pub topology: Topology,
    /// Record name. Never empty and never synthesized; see the accepted model.
    pub name: String,
    /// Optional, nullable description. Always emitted; null when unknown.
    #[serde(default)]
    pub description: Option<String>,
    /// Number of bases or residues; Record::validate() requires it to equal the
    /// sequence length.
    pub length: u64,
    /// Normalized sequence: uppercase IUPAC DNA, no whitespace and no digits.
    pub sequence: String,
    /// The two identities of the record.
    pub digests: Digests,
    /// Features in canonical order (source order, never sorted).
    pub features: Vec<Feature>,
    /// Where the record came from.
    pub provenance: Provenance,
    /// Adapter-namespaced extension payloads. Free-form; never interpreted by
    /// core code and never part of a digest.
    pub extras: Extras,
}
