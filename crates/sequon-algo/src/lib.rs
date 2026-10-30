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
//!
//! The annotation layer (docs/design/annotation-operations.md) applies
//! annotation-only transforms to a validated Record and returns a new validated
//! Record. It never edits nucleotide bases: there is no insert, delete, replace
//! or rebase API anywhere in this crate.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

mod analysis;
mod annotation;
mod digest;
mod error;
mod extract;
mod gc;
mod iupac;
mod normalize;
mod provenance;
mod restrict;
mod revcomp;
mod rotate;
mod translate;

pub use analysis::{
    analyze_record, analyze_record_with_database, analyze_selection, CdsAnalysis, CdsAnalysisError,
    RecordAnalysis, SelectionAnalysis, SelectionOperation,
};
pub use annotation::{
    add_feature, apply_annotation_operations, delete_feature, save_selection_as_cds,
    update_feature, AnnotationError, AnnotationOperation, AnnotationOutcome, AnnotationProvenance,
    FeatureIdentityMap, FeatureInput, OperationIdentity, SaveAsCdsRequest, ANNOTATION_ALGORITHM_ID,
    ANNOTATION_ALGORITHM_VERSION, MAX_ANNOTATION_OPERATIONS, MAX_FEATURES_PER_RECORD,
    MAX_FEATURE_KEY_BYTES, MAX_QUALIFIERS_PER_FEATURE, MAX_QUALIFIER_KEY_BYTES,
    MAX_QUALIFIER_VALUE_BYTES, MAX_SEGMENTS_PER_FEATURE, SAVE_AS_CDS_KEY, SAVE_AS_CDS_SO_TERM,
};
pub use digest::{
    bytes_digest, canonical_digest, sequence_digest, verify_digests, DigestVerificationError,
};
pub use error::AnalysisError;
pub use extract::{
    extract_location, extract_selection, Continuity, CoordinateSegment, ExtractedSegment,
    LocationExtraction, Selection,
};
pub use gc::{gc_content, GcContent};
pub use normalize::{check_normalized, normalize_sequence, SequenceError, IUPAC_DNA};
pub use provenance::{
    same_scientific_content, AlgorithmProducer, AlgorithmVersion, AnalysisProvenance,
    DatabaseVersion,
};
pub use restrict::{
    analyze_restriction, restriction_database, AnchorSide, NavigationAnchor, Overhang,
    OverhangKind, RecognitionRange, RestrictionAmbiguity, RestrictionAmbiguityCode,
    RestrictionAnalysis, RestrictionCut, RestrictionDatabase, RestrictionEnzyme, RestrictionHit,
};
pub use revcomp::reverse_complement;
pub use rotate::min_rotation;
pub use translate::{
    genetic_code_database, translate_cds, translate_location, translate_selection, AminoAcid,
    CdsTranslation, CodonTranslation, SelectionTranslationOptions,
};
