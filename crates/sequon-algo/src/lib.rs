//! Sequence computation for Sequon.
//!
//! Everything here operates on canonical values and has no knowledge of file
//! formats. The dependency direction is model, then algo, then IO, so this crate
//! never pulls in a parser. Functions are deterministic and report invalid
//! input through typed errors rather than panicking.
//!
//! The digest layer implements sequon-digest/1 as accepted in
//! docs/design/canonical-model.md:
//!
//! - the sequence digest is SHA-256 of the normalized sequence in its recorded
//!   orientation, so it is direction-sensitive;
//! - the canonical digest is SHA-256 of the lexicographically smaller of the
//!   sequence and its reverse complement for linear records, and of the
//!   lexicographically smallest rotation of either orientation for circular
//!   records, so it identifies the molecule across rotation and strand;
//! - the algorithm identifier is versioned independently of the parser version,
//!   so upgrading a parser never changes a canonical digest.
//!
//! Normalization is adapter-facing: adapters call normalize_sequence() once,
//! store the normalized sequence, and report what they normalized
//! (GB_SEQUENCE_NORMALIZED or DNA_SEQUENCE_NORMALIZED). The canonical record
//! never stores unnormalized text.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

mod digest;
mod normalize;
mod revcomp;
mod rotate;

pub use digest::{
    bytes_digest, canonical_digest, sequence_digest, verify_digests, DigestVerificationError,
};
pub use normalize::{check_normalized, normalize_sequence, SequenceError, IUPAC_DNA};
pub use revcomp::reverse_complement;
pub use rotate::min_rotation;
