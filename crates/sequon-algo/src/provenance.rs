//! Version and producer metadata attached to every derived result.

use sequon_model::{DigestValue, Record};

use crate::bytes_digest;

/// Sequon producer identity for derived results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlgorithmProducer {
    /// Always sequon-algo.
    pub name: &'static str,
    /// Exact crate package version.
    pub version: &'static str,
}

/// One independently versioned algorithm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlgorithmVersion {
    /// Stable algorithm identifier.
    pub id: String,
    /// Algorithm semantic version within that identifier.
    pub version: String,
}

/// One versioned scientific database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseVersion {
    /// Stable database identifier.
    pub id: String,
    /// Database release identifier.
    pub version: String,
    /// SHA-256 of the deterministic database bytes, formatted as sha256:hex.
    pub sha256: String,
}

/// Provenance attached to one analysis result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisProvenance {
    /// Sequon library producer.
    pub producer: AlgorithmProducer,
    /// Algorithm used for this result.
    pub algorithm: AlgorithmVersion,
    /// Scientific database used, when applicable.
    pub database: Option<DatabaseVersion>,
}

impl AnalysisProvenance {
    pub(crate) fn new(id: &str) -> Self {
        AnalysisProvenance {
            producer: AlgorithmProducer {
                name: "sequon-algo",
                version: env!("CARGO_PKG_VERSION"),
            },
            algorithm: AlgorithmVersion {
                id: id.to_owned(),
                version: "1".to_owned(),
            },
            database: None,
        }
    }

    pub(crate) fn with_database(mut self, database: DatabaseVersion) -> Self {
        self.database = Some(database);
        self
    }
}

pub(crate) fn extraction() -> AnalysisProvenance {
    AnalysisProvenance::new("sequon-extract")
}

pub(crate) fn reverse_complement() -> AnalysisProvenance {
    AnalysisProvenance::new("sequon-revcomp")
}

pub(crate) fn gc() -> AnalysisProvenance {
    AnalysisProvenance::new("sequon-gc")
}

pub(crate) fn translation(database: DatabaseVersion) -> AnalysisProvenance {
    AnalysisProvenance::new("sequon-translation").with_database(database)
}

pub(crate) fn restriction(database: DatabaseVersion) -> AnalysisProvenance {
    AnalysisProvenance::new("sequon-restriction").with_database(database)
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let digest: DigestValue = bytes_digest(bytes);
    digest.to_string()
}

/// Returns whether two records have identical sequence, topology and feature
/// content. Analysis tests use this to prove that no result is written back.
pub fn same_scientific_content(before: &Record, after: &Record) -> bool {
    before.sequence == after.sequence
        && before.length == after.length
        && before.topology == after.topology
        && before.features == after.features
        && before.digests == after.digests
}
