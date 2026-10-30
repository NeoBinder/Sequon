//! Typed failures for derived sequence analysis.

use sequon_model::{Topology, ValidationError};

use crate::{DigestVerificationError, SequenceError};

/// Error returned by an analysis operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalysisError {
    /// The canonical Record failed its cross-field validation.
    RecordValidation(ValidationError),
    /// The canonical Record failed digest verification.
    DigestVerification(DigestVerificationError),
    /// A sequence was not normalized IUPAC DNA.
    Sequence(SequenceError),
    /// An analysis selection had zero length.
    SelectionEmpty,
    /// An analysis selection was outside the topology-aware record bounds.
    SelectionOutOfRange {
        /// Requested start.
        start: u64,
        /// Requested end.
        end: u64,
        /// Record length.
        length: u64,
        /// Record topology.
        topology: Topology,
    },
    /// Selection operations were empty or contained a duplicate.
    InvalidSelectionOperation {
        /// Stable reason suitable for diagnostics.
        reason: &'static str,
    },
    /// A location operator, segment count or strand was invalid.
    InvalidLocation {
        /// Stable reason suitable for diagnostics.
        reason: &'static str,
    },
    /// A location segment was outside the topology-aware record bounds.
    LocationOutOfRange {
        /// Segment index in canonical source order.
        segment_index: usize,
        /// Segment start.
        start: u64,
        /// Segment end.
        end: u64,
        /// Record length.
        length: u64,
        /// Record topology.
        topology: Topology,
    },
    /// A feature-oriented translation was requested for strand zero.
    UnstrandedTranslation,
    /// CDS translation through order's unknown gaps is not permitted.
    OrderCdsUnsupported,
    /// codon_start was duplicated, malformed, outside 1..=3 or beyond the CDS.
    CodonStartInvalid {
        /// Stable reason suitable for diagnostics.
        reason: &'static str,
    },
    /// transl_table was duplicated or was not an integer.
    TranslationTableInvalid {
        /// Stable reason suitable for diagnostics.
        reason: &'static str,
    },
    /// The requested NCBI translation table is outside the supported set 1, 11.
    TranslationTableUnsupported {
        /// Unsupported table number.
        table: u32,
    },
    /// A selection frame was outside 0..=2 or beyond the selection.
    FrameOutOfRange {
        /// Requested frame.
        frame: u8,
        /// Selection length.
        length: u64,
    },
    /// A restriction database entry was inconsistent.
    InvalidRestrictionDatabase {
        /// Stable reason suitable for diagnostics.
        reason: &'static str,
    },
}

impl AnalysisError {
    /// Returns the stable Sequon analysis error code.
    pub fn code(&self) -> &'static str {
        match self {
            AnalysisError::RecordValidation(_) => "RECORD_INVALID",
            AnalysisError::DigestVerification(_) => "DIGEST_MISMATCH",
            AnalysisError::Sequence(_) => "SEQUENCE_INVALID",
            AnalysisError::SelectionEmpty => "SELECTION_EMPTY",
            AnalysisError::SelectionOutOfRange { .. } => "SELECTION_OUT_OF_RANGE",
            AnalysisError::InvalidSelectionOperation { .. } => "SELECTION_OPERATION_INVALID",
            AnalysisError::InvalidLocation { .. } => "LOCATION_INVALID",
            AnalysisError::LocationOutOfRange { .. } => "LOCATION_OUT_OF_RANGE",
            AnalysisError::UnstrandedTranslation => "STRAND_UNSUPPORTED",
            AnalysisError::OrderCdsUnsupported => "ORDER_CDS_UNSUPPORTED",
            AnalysisError::CodonStartInvalid { .. } => "CODON_START_INVALID",
            AnalysisError::TranslationTableInvalid { .. } => "TRANSL_TABLE_INVALID",
            AnalysisError::TranslationTableUnsupported { .. } => "TRANSL_TABLE_UNSUPPORTED",
            AnalysisError::FrameOutOfRange { .. } => "FRAME_OUT_OF_RANGE",
            AnalysisError::InvalidRestrictionDatabase { .. } => "RESTRICTION_DATABASE_INVALID",
        }
    }
}

impl std::fmt::Display for AnalysisError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnalysisError::RecordValidation(error) => write!(formatter, "invalid canonical record: {error}"),
            AnalysisError::DigestVerification(error) => write!(formatter, "digest verification failed: {error}"),
            AnalysisError::Sequence(error) => write!(formatter, "invalid normalized sequence: {error}"),
            AnalysisError::SelectionEmpty => write!(formatter, "analysis selection must not be empty"),
            AnalysisError::SelectionOutOfRange { start, end, length, topology } => write!(
                formatter,
                "selection [{start},{end}) is outside a {topology:?} record of length {length}"
            ),
            AnalysisError::InvalidSelectionOperation { reason } => write!(formatter, "invalid selection operation: {reason}"),
            AnalysisError::InvalidLocation { reason } => write!(formatter, "invalid location: {reason}"),
            AnalysisError::LocationOutOfRange { segment_index, start, end, length, topology } => write!(
                formatter,
                "segment {segment_index} [{start},{end}) is outside a {topology:?} record of length {length}"
            ),
            AnalysisError::UnstrandedTranslation => write!(formatter, "feature-oriented translation requires strand 1 or -1"),
            AnalysisError::OrderCdsUnsupported => write!(formatter, "CDS translation through order unknown gaps is unsupported"),
            AnalysisError::CodonStartInvalid { reason } => write!(formatter, "invalid codon_start: {reason}"),
            AnalysisError::TranslationTableInvalid { reason } => write!(formatter, "invalid transl_table: {reason}"),
            AnalysisError::TranslationTableUnsupported { table } => write!(formatter, "transl_table {table} is unsupported; use 1 or 11"),
            AnalysisError::FrameOutOfRange { frame, length } => write!(formatter, "frame {frame} exceeds selection length {length} or the range 0..=2"),
            AnalysisError::InvalidRestrictionDatabase { reason } => write!(formatter, "invalid restriction database: {reason}"),
        }
    }
}

impl std::error::Error for AnalysisError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AnalysisError::RecordValidation(error) => Some(error),
            AnalysisError::DigestVerification(error) => Some(error),
            AnalysisError::Sequence(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ValidationError> for AnalysisError {
    fn from(error: ValidationError) -> Self {
        AnalysisError::RecordValidation(error)
    }
}

impl From<DigestVerificationError> for AnalysisError {
    fn from(error: DigestVerificationError) -> Self {
        AnalysisError::DigestVerification(error)
    }
}

impl From<SequenceError> for AnalysisError {
    fn from(error: SequenceError) -> Self {
        AnalysisError::Sequence(error)
    }
}

pub(crate) fn validate_record(record: &sequon_model::Record) -> Result<(), AnalysisError> {
    record.validate()?;
    crate::verify_digests(record)?;
    Ok(())
}
