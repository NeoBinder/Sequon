//! Annotation-only canonical transforms: feature add, update, delete and
//! save-selection-as-CDS.
//!
//! This module implements the accepted annotation operation contract
//! (docs/design/annotation-operations.md). It is a pure canonical transform: it
//! never edits nucleotide bases, never accepts or rewrites sequence, length,
//! topology, digests, extras or source lineage, and reads no database, clock,
//! randomness or network.
//!
//! The entry point [apply_annotation_operations] validates the base record,
//! resolves base-version source-order feature identities, validates every
//! operation, applies the whole request atomically to produce a NEW [Record],
//! records the algorithm producer provenance, and validates and digest-verifies
//! the result. Any invalid operation rejects the entire list; there is no
//! partial transform and no silent skip.
//!
//! There is no insert, delete, replace or rebase API here (and none elsewhere in
//! this crate): annotation operations can only change `record.features` and
//! `record.provenance.producer`. The molecule is untouched, so both digests
//! stay valid and the new canonical document differs only by its annotations and
//! producer pin, which is what gives the platform a new immutable version
//! identity.
//!
//! Platform identity is deliberately outside this pure outcome. The immutable
//! base version number, the parent/result `payload_sha256` and the
//! Idempotency-Key belong to the consuming platform: NeoAPI-rs owns them, and it
//! computes the canonical JSON bytes and their hash with the Sequon IO
//! serializer. This crate carries no format or serialization layer (the
//! dependency direction is model, then algorithm, then IO), so it must not
//! re-implement canonical JSON. See section 1.1 of the contract for the
//! implemented surface and that boundary.

use sequon_model::{Feature, Location, Producer, ProducerKind, Qualifier, Record, ValidationError};

use crate::extract::Selection;
use crate::provenance::{AlgorithmProducer, AlgorithmVersion};
use crate::DigestVerificationError;

/// Maximum number of operations in one atomic request.
pub const MAX_ANNOTATION_OPERATIONS: usize = 100;

/// Maximum number of features in one result record.
pub const MAX_FEATURES_PER_RECORD: usize = 100_000;

/// Maximum number of segments in one added or updated feature location.
pub const MAX_SEGMENTS_PER_FEATURE: usize = 1_024;

/// Maximum number of qualifiers on one added or updated feature.
pub const MAX_QUALIFIERS_PER_FEATURE: usize = 256;

/// Maximum size of one qualifier value in UTF-8 bytes (64 KiB).
pub const MAX_QUALIFIER_VALUE_BYTES: usize = 64 * 1024;

/// Maximum size of one feature key in UTF-8 bytes.
///
/// The contract requires a non-empty bounded key without fixing the number; this
/// is the reviewed R11 bound (see the implementation note in the contract).
pub const MAX_FEATURE_KEY_BYTES: usize = 255;

/// Maximum size of one qualifier key in UTF-8 bytes.
///
/// The contract requires a non-empty, bounded, control-character-free key
/// without fixing the number; this is the reviewed R11 bound.
pub const MAX_QUALIFIER_KEY_BYTES: usize = 255;

/// Canonical feature key written by save-selection-as-CDS.
pub const SAVE_AS_CDS_KEY: &str = "CDS";

/// Canonical Sequence Ontology term written by save-selection-as-CDS.
pub const SAVE_AS_CDS_SO_TERM: &str = "SO:0000316";

/// Stable algorithm identifier recorded in the result provenance.
pub const ANNOTATION_ALGORITHM_ID: &str = "sequon-annotation";

/// Algorithm semantic version recorded in the result provenance.
pub const ANNOTATION_ALGORITHM_VERSION: &str = "1";

/// Producer name written into the edited record provenance.
const PRODUCER_NAME: &str = "sequon-algo";

/// Protected qualifier: derived translation, never authored by an edit.
const QUALIFIER_TRANSLATION: &str = "translation";

/// Protected qualifier written exactly once by save-selection-as-CDS.
const QUALIFIER_CODON_START: &str = "codon_start";

/// Protected qualifier written exactly once by save-selection-as-CDS.
const QUALIFIER_TRANSL_TABLE: &str = "transl_table";

// ---------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------

/// One canonical feature supplied by an add or update operation.
///
/// It contains exactly the canonical feature fields: no display colour, template
/// identity, database id, sequence, envelope or provenance field can be
/// supplied. Unknown feature keys and an absent SO term remain verbatim; nothing
/// is guessed from product or gene qualifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureInput {
    /// Feature key exactly as the caller supplies it; never rewritten.
    pub key: String,
    /// Optional Sequence Ontology term; null means not derived.
    pub so_term: Option<String>,
    /// Optional display name; null means none and is never synthesized.
    pub name: Option<String>,
    /// Canonical location with source-order segments.
    pub location: Location,
    /// Ordered qualifier pairs, duplicates and order preserved.
    pub qualifiers: Vec<Qualifier>,
}

impl FeatureInput {
    /// Builds a feature input from canonical values.
    pub fn new(key: impl Into<String>, location: Location, qualifiers: Vec<Qualifier>) -> Self {
        FeatureInput {
            key: key.into(),
            so_term: None,
            name: None,
            location,
            qualifiers,
        }
    }
}

/// The save-selection-as-CDS request body of one operation.
///
/// This is not ORF discovery: start/stop presence is neither required nor
/// invented, and no partial boundary flags are accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveAsCdsRequest {
    /// Positive-length 0-based half-open selection; a circular record may carry
    /// one unwrapped origin-crossing span.
    pub selection: Selection,
    /// 1 (forward) or -1 (reverse complement); 0 is invalid.
    pub strand: i8,
    /// Optional exact feature name; null means none.
    pub name: Option<String>,
    /// Optional reading frame 1, 2 or 3; absent means 1.
    pub codon_start: Option<u8>,
    /// Optional NCBI translation table 1 or 11; absent means 1.
    pub transl_table: Option<u16>,
    /// Ordered qualifiers preserved after the written codon_start/transl_table.
    pub qualifiers: Vec<Qualifier>,
}

/// One ordered annotation operation of one atomic request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnnotationOperation {
    /// Append one caller-supplied canonical feature as the last feature.
    AddFeature(FeatureInput),
    /// Replace the complete base feature at its base source-order index.
    UpdateFeature {
        /// Zero-based source-order index in the base version.
        feature_index: usize,
        /// Complete replacement feature.
        feature: FeatureInput,
    },
    /// Delete the base feature at its base source-order index.
    DeleteFeature {
        /// Zero-based source-order index in the base version.
        feature_index: usize,
    },
    /// Append one CDS span created from a selection.
    SaveAsCds(SaveAsCdsRequest),
}

// ---------------------------------------------------------------------------
// Outcomes
// ---------------------------------------------------------------------------

/// One result identity for an add or save operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationIdentity {
    /// Zero-based index in the request operation list.
    pub operation_index: usize,
    /// Zero-based source-order index in the result record.
    pub result_index: usize,
}

/// In-version feature identity map between the base and result versions.
///
/// Canonical features carry no stable database id: identity is unique only
/// within one immutable version, and this map is what the platform uses to
/// attach its own style/template metadata to the new version. It never rewrites
/// an old version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureIdentityMap {
    /// Number of features in the base record.
    pub base_feature_count: usize,
    /// Number of features in the result record.
    pub result_feature_count: usize,
    /// One entry per base feature in base source order: the result index for a
    /// surviving feature, null for a deleted one.
    pub base_to_result: Vec<Option<usize>>,
    /// One entry per add/save operation in request order.
    pub operation_to_result: Vec<OperationIdentity>,
}

/// Algorithm provenance of one annotation transform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnotationProvenance {
    /// Sequon library producer written into the record.
    pub producer: AlgorithmProducer,
    /// Annotation algorithm identity and semantic version.
    pub algorithm: AlgorithmVersion,
}

/// The complete pure result of one atomic annotation transform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnotationOutcome {
    /// The new validated and digest-verified canonical Record.
    pub record: Record,
    /// Base/result feature identity map.
    pub feature_identity_map: FeatureIdentityMap,
    /// Algorithm and producer provenance of the transform.
    pub provenance: AnnotationProvenance,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// A typed failure that rejects the complete annotation request.
///
/// The platform maps [code](AnnotationError::code) onto its public annotation
/// errors; messages never contain sequence content or raw qualifier values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnnotationError {
    /// The base or result Record failed its cross-field validation.
    RecordValidation(ValidationError),
    /// The base or result Record failed digest verification.
    DigestVerification(DigestVerificationError),
    /// The request contained no operations.
    EmptyOperations,
    /// One documented R11 resource limit was exceeded.
    LimitExceeded {
        /// Limit name, for example max_operations.
        limit: &'static str,
        /// Configured value.
        limit_value: u64,
        /// Observed value.
        actual: u64,
    },
    /// A feature index does not resolve against the expected base version.
    IdentityOutOfRange {
        /// Operation position in the request.
        operation_index: usize,
        /// Requested base source-order index.
        feature_index: usize,
        /// Number of features in the base version.
        base_feature_count: usize,
    },
    /// A base feature was targeted by more than one update or delete.
    IdentityConflict {
        /// Operation position of the conflicting target.
        operation_index: usize,
        /// Base source-order index that appears more than once.
        feature_index: usize,
    },
    /// A feature field is invalid under the canonical model.
    InvalidFeature {
        /// Operation position in the request.
        operation_index: usize,
        /// Stable reason without user values.
        reason: &'static str,
    },
    /// A location is invalid under the canonical model.
    InvalidLocation {
        /// Operation position in the request.
        operation_index: usize,
        /// Stable underlying location error code.
        reason: &'static str,
    },
    /// A save-as-CDS selection is invalid under the canonical selection rules.
    InvalidSelection {
        /// Operation position in the request.
        operation_index: usize,
        /// Stable underlying selection error code.
        reason: &'static str,
    },
    /// A save-as-CDS selection length is not a positive multiple of three.
    CdsLengthInvalid {
        /// Operation position in the request.
        operation_index: usize,
        /// Observed selection length.
        length: u64,
    },
    /// A protected or duplicate qualifier is not allowed by this operation.
    QualifierUnsupported {
        /// Operation position in the request.
        operation_index: usize,
        /// Protected qualifier key.
        key: &'static str,
        /// Stable reason without the supplied value.
        reason: &'static str,
    },
}

impl AnnotationError {
    /// Returns the stable Sequon annotation error code.
    pub fn code(&self) -> &'static str {
        match self {
            AnnotationError::RecordValidation(_) => "RECORD_INVALID",
            AnnotationError::DigestVerification(_) => "DIGEST_MISMATCH",
            AnnotationError::EmptyOperations => "ANNOTATION_FEATURE_INVALID",
            AnnotationError::LimitExceeded { .. } => "ANNOTATION_LIMIT_EXCEEDED",
            AnnotationError::IdentityOutOfRange { .. }
            | AnnotationError::IdentityConflict { .. } => "ANNOTATION_IDENTITY_CONFLICT",
            AnnotationError::InvalidFeature { .. }
            | AnnotationError::InvalidLocation { .. }
            | AnnotationError::InvalidSelection { .. } => "ANNOTATION_FEATURE_INVALID",
            AnnotationError::CdsLengthInvalid { .. } => "ANNOTATION_CDS_LENGTH_INVALID",
            AnnotationError::QualifierUnsupported { .. } => "ANNOTATION_QUALIFIER_UNSUPPORTED",
        }
    }
}

impl std::fmt::Display for AnnotationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnnotationError::RecordValidation(error) => {
                write!(formatter, "invalid canonical record: {error}")
            }
            AnnotationError::DigestVerification(error) => {
                write!(formatter, "digest verification failed: {error}")
            }
            AnnotationError::EmptyOperations => {
                write!(formatter, "an annotation request requires at least one operation")
            }
            AnnotationError::LimitExceeded {
                limit,
                limit_value,
                actual,
            } => write!(
                formatter,
                "annotation limit {limit} exceeded: configured {limit_value}, observed {actual}"
            ),
            AnnotationError::IdentityOutOfRange {
                operation_index,
                feature_index,
                base_feature_count,
            } => write!(
                formatter,
                "operation {operation_index} targets base feature index {feature_index}, but the \
                 base version has {base_feature_count} features"
            ),
            AnnotationError::IdentityConflict {
                operation_index,
                feature_index,
            } => write!(
                formatter,
                "operation {operation_index} targets base feature index {feature_index}, which is \
                 already targeted by another update or delete"
            ),
            AnnotationError::InvalidFeature {
                operation_index,
                reason,
            } => write!(
                formatter,
                "operation {operation_index} supplied an invalid feature: {reason}"
            ),
            AnnotationError::InvalidLocation {
                operation_index,
                reason,
            } => write!(
                formatter,
                "operation {operation_index} supplied an invalid location: {reason}"
            ),
            AnnotationError::InvalidSelection {
                operation_index,
                reason,
            } => write!(
                formatter,
                "operation {operation_index} supplied an invalid selection: {reason}"
            ),
            AnnotationError::CdsLengthInvalid {
                operation_index,
                length,
            } => write!(
                formatter,
                "operation {operation_index} save-as-CDS selection length {length} is not a positive \
                 multiple of three"
            ),
            AnnotationError::QualifierUnsupported {
                operation_index,
                key,
                reason,
            } => write!(
                formatter,
                "operation {operation_index} qualifier '{key}' is not accepted: {reason}"
            ),
        }
    }
}

impl std::error::Error for AnnotationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AnnotationError::RecordValidation(error) => Some(error),
            AnnotationError::DigestVerification(error) => Some(error),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// Applies one atomic annotation-only transform to a validated base Record.
///
/// The base Record is never modified. Every operation is resolved against the
/// base version source order, validated against the canonical model and the R11
/// limits, then applied in one step: updates replace at their base position,
/// deletes remove the identified feature, surviving features keep their relative
/// order, and add/save results append in request order and are reindexed
/// contiguously. Any failure rejects the whole request.
///
/// The result is validated with [Record::validate] and digest-verified with
/// [crate::verify_digests] before it is returned; the base is validated and
/// digest-verified before planning.
pub fn apply_annotation_operations(
    base: &Record,
    operations: &[AnnotationOperation],
) -> Result<AnnotationOutcome, AnnotationError> {
    base.validate().map_err(AnnotationError::RecordValidation)?;
    crate::verify_digests(base).map_err(AnnotationError::DigestVerification)?;

    if operations.is_empty() {
        return Err(AnnotationError::EmptyOperations);
    }
    if operations.len() > MAX_ANNOTATION_OPERATIONS {
        return Err(AnnotationError::LimitExceeded {
            limit: "max_operations",
            limit_value: MAX_ANNOTATION_OPERATIONS as u64,
            actual: operations.len() as u64,
        });
    }
    // The feature-count limit is a limit on the RESULT record (contract section
    // 3), not on the imported base: a request may delete features to get under
    // it. plan() checks the resulting count before anything is assembled.
    let plan = plan(base, operations)?;
    let outcome = assemble(base, plan)?;
    outcome
        .record
        .validate()
        .map_err(AnnotationError::RecordValidation)?;
    crate::verify_digests(&outcome.record).map_err(AnnotationError::DigestVerification)?;
    Ok(outcome)
}

/// Appends one caller-supplied canonical feature.
pub fn add_feature(
    base: &Record,
    feature: FeatureInput,
) -> Result<AnnotationOutcome, AnnotationError> {
    apply_annotation_operations(base, &[AnnotationOperation::AddFeature(feature)])
}

/// Replaces the complete base feature at one base source-order index.
pub fn update_feature(
    base: &Record,
    feature_index: usize,
    feature: FeatureInput,
) -> Result<AnnotationOutcome, AnnotationError> {
    apply_annotation_operations(
        base,
        &[AnnotationOperation::UpdateFeature {
            feature_index,
            feature,
        }],
    )
}

/// Deletes the base feature at one base source-order index.
pub fn delete_feature(
    base: &Record,
    feature_index: usize,
) -> Result<AnnotationOutcome, AnnotationError> {
    apply_annotation_operations(
        base,
        &[AnnotationOperation::DeleteFeature { feature_index }],
    )
}

/// Appends one CDS span created from a selection (not ORF discovery).
pub fn save_selection_as_cds(
    base: &Record,
    request: SaveAsCdsRequest,
) -> Result<AnnotationOutcome, AnnotationError> {
    apply_annotation_operations(base, &[AnnotationOperation::SaveAsCds(request)])
}

// ---------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------

/// The validated plan of one request, before any record is built.
struct Plan {
    updates: Vec<Option<Feature>>,
    deleted: Vec<bool>,
    appends: Vec<(usize, Feature)>,
}

fn plan(base: &Record, operations: &[AnnotationOperation]) -> Result<Plan, AnnotationError> {
    let base_feature_count = base.features.len();
    let mut updates: Vec<Option<Feature>> = (0..base_feature_count).map(|_| None).collect();
    let mut deleted = vec![false; base_feature_count];
    let mut targeted: Vec<Option<usize>> = vec![None; base_feature_count];
    let mut appends: Vec<(usize, Feature)> = Vec::new();

    for (operation_index, operation) in operations.iter().enumerate() {
        match operation {
            AnnotationOperation::AddFeature(input) => {
                let feature = validate_feature_input(base, operation_index, input)?;
                reject_translation(
                    operation_index,
                    &feature.qualifiers,
                    "an add operation must not introduce a derived /translation",
                )?;
                appends.push((operation_index, feature));
            }
            AnnotationOperation::UpdateFeature {
                feature_index,
                feature,
            } => {
                resolve_target(
                    base_feature_count,
                    operation_index,
                    *feature_index,
                    &mut targeted,
                )?;
                let replacement = validate_feature_input(base, operation_index, feature)?;
                let base_feature = &base.features[*feature_index];
                require_same_translation(operation_index, base_feature, &replacement)?;
                updates[*feature_index] = Some(replacement);
            }
            AnnotationOperation::DeleteFeature { feature_index } => {
                resolve_target(
                    base_feature_count,
                    operation_index,
                    *feature_index,
                    &mut targeted,
                )?;
                deleted[*feature_index] = true;
            }
            AnnotationOperation::SaveAsCds(request) => {
                let feature = build_selection_cds(base, operation_index, request)?;
                appends.push((operation_index, feature));
            }
        }
    }

    let surviving = base_feature_count - deleted.iter().filter(|removed| **removed).count();
    let result_feature_count = surviving + appends.len();
    if result_feature_count > MAX_FEATURES_PER_RECORD {
        return Err(AnnotationError::LimitExceeded {
            limit: "max_features_per_record",
            limit_value: MAX_FEATURES_PER_RECORD as u64,
            actual: result_feature_count as u64,
        });
    }

    Ok(Plan {
        updates,
        deleted,
        appends,
    })
}

/// Resolves one base source-order target and rejects a duplicate target.
fn resolve_target(
    base_feature_count: usize,
    operation_index: usize,
    feature_index: usize,
    targeted: &mut [Option<usize>],
) -> Result<(), AnnotationError> {
    if feature_index >= base_feature_count {
        return Err(AnnotationError::IdentityOutOfRange {
            operation_index,
            feature_index,
            base_feature_count,
        });
    }
    if targeted[feature_index].is_some() {
        return Err(AnnotationError::IdentityConflict {
            operation_index,
            feature_index,
        });
    }
    targeted[feature_index] = Some(operation_index);
    Ok(())
}

// ---------------------------------------------------------------------------
// Feature validation
// ---------------------------------------------------------------------------

fn validate_feature_input(
    base: &Record,
    operation_index: usize,
    input: &FeatureInput,
) -> Result<Feature, AnnotationError> {
    if input.key.is_empty() {
        return Err(AnnotationError::InvalidFeature {
            operation_index,
            reason: "feature key must not be empty",
        });
    }
    if input.key.len() > MAX_FEATURE_KEY_BYTES {
        return Err(AnnotationError::InvalidFeature {
            operation_index,
            reason: "feature key exceeds the R11 key bound",
        });
    }
    crate::extract::validate_location(&input.location, base).map_err(|error| {
        AnnotationError::InvalidLocation {
            operation_index,
            reason: error.code(),
        }
    })?;
    if input.location.segments.len() > MAX_SEGMENTS_PER_FEATURE {
        return Err(AnnotationError::LimitExceeded {
            limit: "max_segments_per_feature",
            limit_value: MAX_SEGMENTS_PER_FEATURE as u64,
            actual: input.location.segments.len() as u64,
        });
    }
    validate_qualifiers(operation_index, &input.qualifiers)?;
    Ok(Feature {
        key: input.key.clone(),
        so_term: input.so_term.clone(),
        name: input.name.clone(),
        location: input.location.clone(),
        qualifiers: input.qualifiers.clone(),
    })
}

/// Validates the ordered qualifier list against the canonical model and R11
/// limits.
fn validate_qualifiers(
    operation_index: usize,
    qualifiers: &[Qualifier],
) -> Result<(), AnnotationError> {
    if qualifiers.len() > MAX_QUALIFIERS_PER_FEATURE {
        return Err(AnnotationError::LimitExceeded {
            limit: "max_qualifiers_per_feature",
            limit_value: MAX_QUALIFIERS_PER_FEATURE as u64,
            actual: qualifiers.len() as u64,
        });
    }
    for qualifier in qualifiers {
        if qualifier.key.is_empty() {
            return Err(AnnotationError::InvalidFeature {
                operation_index,
                reason: "qualifier key must not be empty",
            });
        }
        if qualifier.key.len() > MAX_QUALIFIER_KEY_BYTES {
            return Err(AnnotationError::InvalidFeature {
                operation_index,
                reason: "qualifier key exceeds the R11 key bound",
            });
        }
        if qualifier.key.chars().any(char::is_control) {
            return Err(AnnotationError::InvalidFeature {
                operation_index,
                reason: "qualifier key must not contain control characters",
            });
        }
        if let Some(value) = &qualifier.value {
            if value.len() > MAX_QUALIFIER_VALUE_BYTES {
                return Err(AnnotationError::LimitExceeded {
                    limit: "max_qualifier_value_bytes",
                    limit_value: MAX_QUALIFIER_VALUE_BYTES as u64,
                    actual: value.len() as u64,
                });
            }
        }
    }
    Ok(())
}

/// Rejects a /translation entry where the operation must not author one.
fn reject_translation(
    operation_index: usize,
    qualifiers: &[Qualifier],
    reason: &'static str,
) -> Result<(), AnnotationError> {
    if qualifiers
        .iter()
        .any(|qualifier| qualifier.key == QUALIFIER_TRANSLATION)
    {
        return Err(AnnotationError::QualifierUnsupported {
            operation_index,
            key: QUALIFIER_TRANSLATION,
            reason,
        });
    }
    Ok(())
}

/// An update must not introduce, remove or change the source /translation
/// metadata: its ordered translation entries must equal the base feature's.
fn require_same_translation(
    operation_index: usize,
    base_feature: &Feature,
    replacement: &Feature,
) -> Result<(), AnnotationError> {
    let base_entries: Vec<&Qualifier> = base_feature
        .qualifiers
        .iter()
        .filter(|qualifier| qualifier.key == QUALIFIER_TRANSLATION)
        .collect();
    let replacement_entries: Vec<&Qualifier> = replacement
        .qualifiers
        .iter()
        .filter(|qualifier| qualifier.key == QUALIFIER_TRANSLATION)
        .collect();
    if base_entries != replacement_entries {
        return Err(AnnotationError::QualifierUnsupported {
            operation_index,
            key: QUALIFIER_TRANSLATION,
            reason: "an update must preserve the base /translation entries exactly",
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// save-selection-as-CDS
// ---------------------------------------------------------------------------

fn build_selection_cds(
    base: &Record,
    operation_index: usize,
    request: &SaveAsCdsRequest,
) -> Result<Feature, AnnotationError> {
    if !matches!(request.strand, -1 | 1) {
        return Err(AnnotationError::InvalidFeature {
            operation_index,
            reason: "save-as-CDS strand must be 1 or -1",
        });
    }
    crate::extract::validate_selection(base, request.selection).map_err(|error| {
        AnnotationError::InvalidSelection {
            operation_index,
            reason: error.code(),
        }
    })?;
    let length = request.selection.end - request.selection.start;
    if !length.is_multiple_of(3) {
        return Err(AnnotationError::CdsLengthInvalid {
            operation_index,
            length,
        });
    }
    let codon_start = request.codon_start.unwrap_or(1);
    if !(1..=3).contains(&codon_start) {
        return Err(AnnotationError::InvalidFeature {
            operation_index,
            reason: "codon_start must be 1, 2 or 3",
        });
    }
    let transl_table = request.transl_table.unwrap_or(1);
    if !matches!(transl_table, 1 | 11) {
        return Err(AnnotationError::InvalidFeature {
            operation_index,
            reason: "transl_table must be 1 or 11",
        });
    }
    validate_qualifiers(operation_index, &request.qualifiers)?;
    if request.qualifiers.len() + 2 > MAX_QUALIFIERS_PER_FEATURE {
        return Err(AnnotationError::LimitExceeded {
            limit: "max_qualifiers_per_feature",
            limit_value: MAX_QUALIFIERS_PER_FEATURE as u64,
            actual: (request.qualifiers.len() + 2) as u64,
        });
    }
    for qualifier in &request.qualifiers {
        let protected = match qualifier.key.as_str() {
            QUALIFIER_TRANSLATION => Some((
                QUALIFIER_TRANSLATION,
                "save-as-CDS must not carry a derived /translation",
            )),
            QUALIFIER_CODON_START => Some((
                QUALIFIER_CODON_START,
                "save-as-CDS writes /codon_start exactly once",
            )),
            QUALIFIER_TRANSL_TABLE => Some((
                QUALIFIER_TRANSL_TABLE,
                "save-as-CDS writes /transl_table exactly once",
            )),
            _ => None,
        };
        if let Some((key, reason)) = protected {
            return Err(AnnotationError::QualifierUnsupported {
                operation_index,
                key,
                reason,
            });
        }
    }

    let mut qualifiers = request.qualifiers.clone();
    qualifiers.push(Qualifier::new(
        QUALIFIER_CODON_START,
        codon_start.to_string(),
    ));
    qualifiers.push(Qualifier::new(
        QUALIFIER_TRANSL_TABLE,
        transl_table.to_string(),
    ));

    Ok(Feature {
        key: SAVE_AS_CDS_KEY.to_owned(),
        so_term: Some(SAVE_AS_CDS_SO_TERM.to_owned()),
        name: request.name.clone(),
        location: Location::span(
            request.strand,
            request.selection.start,
            request.selection.end,
        ),
        qualifiers,
    })
}

// ---------------------------------------------------------------------------
// Assembly
// ---------------------------------------------------------------------------

fn assemble(base: &Record, plan: Plan) -> Result<AnnotationOutcome, AnnotationError> {
    let base_feature_count = base.features.len();
    let mut features: Vec<Feature> = Vec::with_capacity(base_feature_count + plan.appends.len());
    let mut base_to_result: Vec<Option<usize>> = vec![None; base_feature_count];

    for (base_index, slot) in base_to_result.iter_mut().enumerate() {
        if plan.deleted[base_index] {
            continue;
        }
        let feature = match &plan.updates[base_index] {
            Some(replacement) => replacement.clone(),
            None => base.features[base_index].clone(),
        };
        *slot = Some(features.len());
        features.push(feature);
    }

    let mut operation_to_result = Vec::with_capacity(plan.appends.len());
    for (operation_index, feature) in plan.appends {
        operation_to_result.push(OperationIdentity {
            operation_index,
            result_index: features.len(),
        });
        features.push(feature);
    }

    let mut record = base.clone();
    record.features = features;
    record.provenance.producer = Producer {
        kind: ProducerKind::Algorithm,
        name: PRODUCER_NAME.to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
    };

    let result_feature_count = record.features.len();
    Ok(AnnotationOutcome {
        record,
        feature_identity_map: FeatureIdentityMap {
            base_feature_count,
            result_feature_count,
            base_to_result,
            operation_to_result,
        },
        provenance: AnnotationProvenance {
            producer: AlgorithmProducer {
                name: PRODUCER_NAME,
                version: env!("CARGO_PKG_VERSION"),
            },
            algorithm: AlgorithmVersion {
                id: ANNOTATION_ALGORITHM_ID.to_owned(),
                version: ANNOTATION_ALGORITHM_VERSION.to_owned(),
            },
        },
    })
}
