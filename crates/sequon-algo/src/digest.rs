//! SHA-256 digests and the sequon-digest/1 equivalence classes.
//!
//! Two values are produced for every record:
//!
//! - sequence_digest() hashes the normalized sequence exactly as recorded, so
//!   it changes when the molecule is entered in the other orientation;
//! - canonical_digest() hashes the equivalence-class representative, so it does
//!   not change under reverse complement for linear records, or under rotation
//!   and reverse complement for circular records.
//!
//! The canonical digest depends only on the normalized sequence, the topology
//! and the algorithm identifier. It never depends on the parser version, the
//! JSON field order, extras or caller hints.

use sequon_model::{DigestValue, Record, Topology, DIGEST_ALGORITHM_V1};
use sha2::{Digest as _, Sha256};

use crate::normalize::{check_normalized, SequenceError};
use crate::revcomp::reverse_complement;
use crate::rotate::min_rotation;

/// Computes the direction-sensitive digest of a normalized sequence.
///
/// The input must already be normalized (uppercase IUPAC DNA); use
/// normalize_sequence() first. A lowercase or otherwise invalid sequence is
/// rejected, so a record cannot silently carry a digest of unnormalized text.
pub fn sequence_digest(sequence: &str) -> Result<DigestValue, SequenceError> {
    check_normalized(sequence)?;
    Ok(hash(sequence.as_bytes()))
}

/// Computes the SHA-256 digest of arbitrary source bytes.
///
/// Used for provenance.source_digest and import-report.source_digest, where the
/// input is a whole file rather than a normalized sequence. The bytes are not
/// validated: any byte string has a digest.
pub fn bytes_digest(bytes: &[u8]) -> DigestValue {
    hash(bytes)
}

/// Computes the canonical digest of a normalized sequence for a topology.
///
/// Linear records use the lexicographically smaller of the sequence and its
/// reverse complement. Circular records use the lexicographically smaller of
/// the smallest rotation of the sequence and the smallest rotation of its
/// reverse complement. The recorded sequence direction is never changed; only
/// the representative used for hashing is chosen.
pub fn canonical_digest(sequence: &str, topology: Topology) -> Result<DigestValue, SequenceError> {
    let reverse = reverse_complement(sequence)?;
    let representative = match topology {
        Topology::Linear => {
            if sequence <= reverse.as_str() {
                sequence.to_owned()
            } else {
                reverse
            }
        }
        Topology::Circular => {
            let forward_rotation = min_rotation(sequence);
            let reverse_rotation = min_rotation(&reverse);
            if forward_rotation <= reverse_rotation {
                forward_rotation
            } else {
                reverse_rotation
            }
        }
    };
    Ok(hash(representative.as_bytes()))
}

/// Hashes bytes with SHA-256 and formats the canonical digest value.
fn hash(bytes: &[u8]) -> DigestValue {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let mut output = [0_u8; 32];
    output.copy_from_slice(&hasher.finalize());
    DigestValue::from_sha256_bytes(output)
}

/// Error returned when a record's stored digests are not trustworthy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DigestVerificationError {
    /// The record names a digest algorithm that version 1 does not define.
    UnsupportedAlgorithm {
        /// Algorithm identifier stored on the record.
        found: String,
    },
    /// The record sequence is not normalized, so no digest can be recomputed.
    InvalidSequence(SequenceError),
    /// The stored sequence digest differs from the recomputed value.
    SequenceMismatch {
        /// Digest stored on the record.
        stored: DigestValue,
        /// Digest recomputed from the record sequence.
        computed: DigestValue,
    },
    /// The stored canonical digest differs from the recomputed value.
    CanonicalMismatch {
        /// Digest stored on the record.
        stored: DigestValue,
        /// Digest recomputed from the record sequence and topology.
        computed: DigestValue,
    },
}

impl From<SequenceError> for DigestVerificationError {
    fn from(error: SequenceError) -> Self {
        DigestVerificationError::InvalidSequence(error)
    }
}

impl std::fmt::Display for DigestVerificationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DigestVerificationError::UnsupportedAlgorithm { found } => write!(
                formatter,
                "record names digest algorithm '{found}'; version 1 understands only '{DIGEST_ALGORITHM_V1}'"
            ),
            DigestVerificationError::InvalidSequence(error) => {
                write!(formatter, "cannot recompute digests: {error}")
            }
            DigestVerificationError::SequenceMismatch { stored, computed } => write!(
                formatter,
                "stored sequence digest {stored} does not match the recomputed digest {computed}"
            ),
            DigestVerificationError::CanonicalMismatch { stored, computed } => write!(
                formatter,
                "stored canonical digest {stored} does not match the recomputed digest {computed}"
            ),
        }
    }
}

impl std::error::Error for DigestVerificationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DigestVerificationError::InvalidSequence(error) => Some(error),
            _ => None,
        }
    }
}

/// Recomputes both digests of a record and compares them with the stored values.
///
/// This is the value-level digest check that Record::validate() cannot perform,
/// because the model does not hash. It does not validate the rest of the record;
/// call Record::validate() for the cross-field invariants. Comparison failures
/// report only digest strings, never sequence content.
pub fn verify_digests(record: &Record) -> Result<(), DigestVerificationError> {
    if record.digests.algorithm != DIGEST_ALGORITHM_V1 {
        return Err(DigestVerificationError::UnsupportedAlgorithm {
            found: record.digests.algorithm.clone(),
        });
    }

    let computed_sequence = sequence_digest(&record.sequence)?;
    if computed_sequence != record.digests.sequence {
        return Err(DigestVerificationError::SequenceMismatch {
            stored: record.digests.sequence.clone(),
            computed: computed_sequence,
        });
    }

    let computed_canonical = canonical_digest(&record.sequence, record.topology)?;
    if computed_canonical != record.digests.canonical {
        return Err(DigestVerificationError::CanonicalMismatch {
            stored: record.digests.canonical.clone(),
            computed: computed_canonical,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sequon_model::{
        DigestValue, Digests, Extras, Kind, Producer, ProducerKind, Provenance, Record,
        SourceFormat, Topology, DIGEST_ALGORITHM_V1,
    };

    // 72 bp synthetic sequence used by the accepted canonical-model example.
    const CONTRACT_SEQUENCE: &str =
        "ATGAGCAAAGGTGAAGAACTGTTTACCGGTGTGGTGCCGATCCTGGTGGAACTGGATGGTAACGGTAAATAA";

    fn digest(text: &str) -> DigestValue {
        DigestValue::parse(text).expect("fixture digest must be canonical")
    }

    fn sample_record(sequence: &str, topology: Topology) -> Record {
        Record {
            schema: "sequon/record/1".to_owned(),
            kind: Kind::Dna,
            molecule_type: None,
            topology,
            name: "sample".to_owned(),
            description: None,
            length: sequence.len() as u64,
            sequence: sequence.to_owned(),
            digests: Digests {
                algorithm: DIGEST_ALGORITHM_V1.to_owned(),
                sequence: sequence_digest(sequence).expect("normalized fixture"),
                canonical: canonical_digest(sequence, topology).expect("normalized fixture"),
            },
            features: Vec::new(),
            provenance: Provenance {
                producer: Producer {
                    kind: ProducerKind::Parser,
                    name: "sequon-io".to_owned(),
                    version: "0.1.0".to_owned(),
                },
                source_format: SourceFormat::Genbank,
                source_digest: digest(
                    "sha256:0000000000000000000000000000000000000000000000000000000000000000",
                ),
                source_bytes: 0,
                export_version: None,
            },
            extras: Extras::new(),
        }
    }

    #[test]
    fn contract_model_example_matches_the_accepted_document() {
        let expected_sequence =
            digest("sha256:8d1653fb92457a81ac44417b35fcd789ec09ad0b60e6cbc687d63e9e6d4c4277");
        let expected_canonical =
            digest("sha256:1cf59a87dc7cd3ad98b286e2b561a4e7820573ce91692fabdb297c106b4fd4b1");
        assert_eq!(CONTRACT_SEQUENCE.len(), 72);
        assert_eq!(
            sequence_digest(CONTRACT_SEQUENCE).expect("normalized"),
            expected_sequence
        );
        assert_eq!(
            canonical_digest(CONTRACT_SEQUENCE, Topology::Circular).expect("normalized"),
            expected_canonical
        );
    }

    #[test]
    fn linear_canonical_digest_uses_the_smaller_orientation() {
        // Reverse complement of the recorded sequence sorts first here, so the
        // canonical digest equals the reverse complement's sequence digest.
        let sequence = "TGCAAGCTTGCAAGCTTGCAAGCTT";
        let reverse = reverse_complement(sequence).expect("normalized");
        assert!(reverse.as_str() < sequence);

        let canonical = canonical_digest(sequence, Topology::Linear).expect("normalized");
        assert_eq!(canonical, sequence_digest(&reverse).expect("normalized"));
        assert_eq!(
            canonical.as_str(),
            "sha256:9eb3ac2d939b083afea3379bd41902a930951eff1cf20355ede53fac7f917a20"
        );

        let sequence_side = sequence_digest(sequence).expect("normalized");
        assert_eq!(
            sequence_side.as_str(),
            "sha256:e03e77d66b5264f3866f986591d4caadc045f03d0e6ba7de1a38ebe5402f9582"
        );
        assert_ne!(canonical, sequence_side);
    }

    #[test]
    fn circular_canonical_digest_is_rotation_and_strand_invariant() {
        let sequence = "GATCCTAGCTAGGCTAAGCTTACCGAT";
        let canonical = canonical_digest(sequence, Topology::Circular).expect("normalized");
        let sequence_side = sequence_digest(sequence).expect("normalized");

        let characters: Vec<char> = sequence.chars().collect();
        for start in 0..characters.len() {
            let rotated: String = characters[start..]
                .iter()
                .chain(characters[..start].iter())
                .collect();
            assert_eq!(
                canonical_digest(&rotated, Topology::Circular).expect("normalized"),
                canonical,
                "rotation at {start} must keep the canonical digest"
            );
            if rotated != sequence {
                assert_ne!(
                    sequence_digest(&rotated).expect("normalized"),
                    sequence_side,
                    "a non-identity rotation at {start} must change the sequence digest"
                );
            }
        }

        let reverse = reverse_complement(sequence).expect("normalized");
        assert_eq!(
            canonical_digest(&reverse, Topology::Circular).expect("normalized"),
            canonical
        );
        assert_ne!(
            sequence_digest(&reverse).expect("normalized"),
            sequence_side
        );
    }

    #[test]
    fn digest_functions_reject_non_normalized_input() {
        assert!(matches!(sequence_digest("acgt"), Err(SequenceError { .. })));
        assert!(canonical_digest("ACGU", Topology::Linear).is_err());
    }

    #[test]
    fn verify_digests_accepts_a_consistent_record() {
        let record = sample_record(CONTRACT_SEQUENCE, Topology::Circular);
        assert!(verify_digests(&record).is_ok());
    }

    #[test]
    fn verify_digests_detects_a_changed_sequence_and_a_changed_canonical_digest() {
        let mut record = sample_record("ATGCGTACGTAGCTAGCTAGCATCGATCGA", Topology::Linear);
        record.sequence = "ATGCGTACGTAGCTAGCTAGCATCGATCGAT".to_owned();
        record.length = record.sequence.len() as u64;
        assert!(matches!(
            verify_digests(&record),
            Err(DigestVerificationError::SequenceMismatch { .. })
        ));

        let mut record = sample_record("ATGCGTACGTAGCTAGCTAGCATCGATCGA", Topology::Linear);
        record.digests.canonical =
            digest("sha256:1111111111111111111111111111111111111111111111111111111111111111");
        assert!(matches!(
            verify_digests(&record),
            Err(DigestVerificationError::CanonicalMismatch { .. })
        ));
    }

    #[test]
    fn verify_digests_detects_an_unknown_algorithm_and_invalid_sequence() {
        let mut record = sample_record("ATGCGTACGTAGCTAGCTAGCATCGATCGA", Topology::Linear);
        record.digests.algorithm = "sequon-digest/2".to_owned();
        assert!(matches!(
            verify_digests(&record),
            Err(DigestVerificationError::UnsupportedAlgorithm { .. })
        ));

        let mut record = sample_record("ATGCGTACGTAGCTAGCTAGCATCGATCGA", Topology::Linear);
        record.sequence = "atgcgtacgtagctagctagcatcgatcga".to_owned();
        assert!(matches!(
            verify_digests(&record),
            Err(DigestVerificationError::InvalidSequence(_))
        ));
    }

    #[test]
    fn verification_errors_do_not_contain_sequence_content() {
        let record = sample_record("ATGCGTACGTAGCTAGCTAGCATCGATCGA", Topology::Linear);
        let reverse = reverse_complement(&record.sequence).expect("normalized");
        let mut tampered = record.clone();
        tampered.digests.sequence = sequence_digest(&reverse).expect("normalized");
        let error = verify_digests(&tampered).expect_err("digests must differ");
        let message = error.to_string();
        assert!(!message.contains(&record.sequence));
        assert!(message.contains("sha256:"));
    }
}
