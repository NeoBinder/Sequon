//! Cross-field validation of canonical records.
//!
//! JSON Schema validates the document shape, enums and patterns. It cannot
//! compare two fields with each other, so Record::validate() owns the
//! invariants that relate fields:
//!
//! - the schema identifier is the version 1 record identifier;
//! - the name and the producer fields are not empty;
//! - length is at least 1 and equals the sequence length;
//! - the sequence is a non-empty uppercase IUPAC DNA string;
//! - the digest algorithm is sequon-digest/1 and both digest strings are
//!   syntactically canonical;
//! - every segment satisfies the topology-aware bounds rule: on a linear record
//!   0 <= start <= end <= length; on a circular record a zero-length site keeps
//!   0 <= start = end <= length while a spanning segment obeys 0 <= start <
//!   length and end <= start + length (end may exceed length by at most one
//!   wrap). span has exactly one segment and join/order have at least one;
//! - strand is -1, 0 or 1 and the operator is one of span, join or order;
//! - feature keys and qualifier keys are not empty.
//!
//! The topology-aware segment rule is the 2026-10-23 revision of the accepted
//! model for circular records: the canonical payload itself stores an unwrapped
//! origin-crossing span as one segment, and an explicitly split source stays
//! split. Linear records are unchanged.
//!
//! The digest values themselves are not recomputed here. Verifying a digest
//! requires hashing, and the model does not depend on a hashing crate; that
//! check belongs to sequon_algo::verify_digests().

use std::error::Error;
use std::fmt;

use crate::digest::DIGEST_ALGORITHM_V1;
use crate::feature::Feature;
use crate::location::{Segment, OPERATOR_JOIN, OPERATOR_ORDER, OPERATOR_SPAN};
use crate::record::{Record, Topology, SCHEMA_ID};

/// Uppercase IUPAC DNA alphabet accepted in a canonical sequence.
const IUPAC_DNA: &str = "ACGTNRYSWKMBDHV";

/// Error naming the field that failed validation and why.
///
/// The path is a dotted field path, for example features\[2\].location.strand.
/// message never contains sequence content beyond the single offending
/// character, and never contains source-file text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    path: String,
    message: String,
}

impl ValidationError {
    pub(crate) fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        ValidationError {
            path: path.into(),
            message: message.into(),
        }
    }

    /// Returns the dotted path of the failing field.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the human-readable explanation.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path, self.message)
    }
}

impl Error for ValidationError {}

impl Record {
    /// Validates the cross-field invariants of this record.
    ///
    /// Call it after import and before persisting. It returns the first failure
    /// as a typed error that names the field path. Structural JSON validity is
    /// a separate concern owned by the JSON Schema and by serde.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_record(self)
    }
}

fn validate_record(record: &Record) -> Result<(), ValidationError> {
    if record.schema != SCHEMA_ID {
        return Err(ValidationError::new(
            "schema",
            format!("expected '{SCHEMA_ID}', found '{}'", record.schema),
        ));
    }

    if record.name.is_empty() {
        return Err(ValidationError::new("name", "must not be empty"));
    }

    if record.length == 0 {
        return Err(ValidationError::new("length", "must be at least 1"));
    }

    let sequence_length = record.sequence.len() as u64;
    if record.length != sequence_length {
        return Err(ValidationError::new(
            "length",
            format!(
                "must equal the sequence length {sequence_length}, found {}",
                record.length
            ),
        ));
    }

    validate_sequence(&record.sequence)?;

    if record.digests.algorithm != DIGEST_ALGORITHM_V1 {
        return Err(ValidationError::new(
            "digests.algorithm",
            format!(
                "must be '{DIGEST_ALGORITHM_V1}', found '{}'",
                record.digests.algorithm
            ),
        ));
    }

    if !record.digests.sequence.is_canonical_form() {
        return Err(ValidationError::new(
            "digests.sequence",
            "must be 'sha256:' followed by 64 lowercase hex digits",
        ));
    }

    if !record.digests.canonical.is_canonical_form() {
        return Err(ValidationError::new(
            "digests.canonical",
            "must be 'sha256:' followed by 64 lowercase hex digits",
        ));
    }

    for (index, feature) in record.features.iter().enumerate() {
        validate_feature(index, feature, record.length, record.topology)?;
    }

    if record.provenance.producer.name.is_empty() {
        return Err(ValidationError::new(
            "provenance.producer.name",
            "must not be empty",
        ));
    }

    if record.provenance.producer.version.is_empty() {
        return Err(ValidationError::new(
            "provenance.producer.version",
            "must not be empty",
        ));
    }

    if !record.provenance.source_digest.is_canonical_form() {
        return Err(ValidationError::new(
            "provenance.source_digest",
            "must be 'sha256:' followed by 64 lowercase hex digits",
        ));
    }

    Ok(())
}

fn validate_sequence(sequence: &str) -> Result<(), ValidationError> {
    if sequence.is_empty() {
        return Err(ValidationError::new("sequence", "must not be empty"));
    }
    for (index, character) in sequence.chars().enumerate() {
        if !IUPAC_DNA.contains(character) {
            return Err(ValidationError::new(
                "sequence",
                format!(
                    "contains '{character}' at character index {index}; only uppercase IUPAC DNA ({IUPAC_DNA}) is allowed"
                ),
            ));
        }
    }
    Ok(())
}

fn validate_feature(
    index: usize,
    feature: &Feature,
    record_length: u64,
    topology: Topology,
) -> Result<(), ValidationError> {
    let path = format!("features[{index}]");

    if feature.key.is_empty() {
        return Err(ValidationError::new(
            format!("{path}.key"),
            "must not be empty",
        ));
    }

    let location = &feature.location;
    let operator = location.operator.as_str();
    if operator != OPERATOR_SPAN && operator != OPERATOR_JOIN && operator != OPERATOR_ORDER {
        return Err(ValidationError::new(
            format!("{path}.location.operator"),
            format!(
                "must be one of '{OPERATOR_SPAN}', '{OPERATOR_JOIN}' or '{OPERATOR_ORDER}', found '{operator}'"
            ),
        ));
    }

    if !matches!(location.strand, -1..=1) {
        return Err(ValidationError::new(
            format!("{path}.location.strand"),
            format!("must be -1, 0 or 1, found {}", location.strand),
        ));
    }

    if location.segments.is_empty() {
        return Err(ValidationError::new(
            format!("{path}.location.segments"),
            "must contain at least one segment",
        ));
    }

    if operator == OPERATOR_SPAN && location.segments.len() != 1 {
        return Err(ValidationError::new(
            format!("{path}.location.segments"),
            format!(
                "operator '{OPERATOR_SPAN}' requires exactly one segment, found {}",
                location.segments.len()
            ),
        ));
    }

    for (segment_index, segment) in location.segments.iter().enumerate() {
        let segment_path = format!("{path}.location.segments[{segment_index}]");
        if segment.start > segment.end {
            return Err(ValidationError::new(
                format!("{segment_path}.start"),
                format!(
                    "must not be greater than end {} (0-based half-open segment)",
                    segment.end
                ),
            ));
        }
        validate_segment_bounds(&segment_path, segment, record_length, topology)?;
    }

    for (qualifier_index, qualifier) in feature.qualifiers.iter().enumerate() {
        if qualifier.key.is_empty() {
            return Err(ValidationError::new(
                format!("{path}.qualifiers[{qualifier_index}].key"),
                "must not be empty",
            ));
        }
    }

    Ok(())
}

/// Checks one segment against the topology-aware bounds rule.
///
/// The caller has already rejected `start > end`. On a linear record every
/// segment stays inside the sequence (`end <= length`). On a circular record of
/// length L the canonical payload may store one unwrapped origin-crossing span,
/// so a spanning segment obeys `start < L` and `end <= start + L` (at most one
/// wrap), while a zero-length between-positions site keeps `start = end <= L`.
/// An explicitly split source is validated segment by segment under the same
/// per-segment rule, which is why a `join` across the origin stays legal.
fn validate_segment_bounds(
    segment_path: &str,
    segment: &Segment,
    record_length: u64,
    topology: Topology,
) -> Result<(), ValidationError> {
    if topology == Topology::Linear {
        if segment.end > record_length {
            return Err(ValidationError::new(
                format!("{segment_path}.end"),
                format!("must not be greater than the record length {record_length}"),
            ));
        }
        return Ok(());
    }

    if segment.start == segment.end {
        if segment.end > record_length {
            return Err(ValidationError::new(
                format!("{segment_path}.end"),
                format!(
                    "must not be greater than the record length {record_length} for a zero-length site on a circular record"
                ),
            ));
        }
        return Ok(());
    }

    if segment.start >= record_length {
        return Err(ValidationError::new(
            format!("{segment_path}.start"),
            format!(
                "must be less than the circular record length {record_length} for a spanning segment"
            ),
        ));
    }

    let circular_bound = segment.start.saturating_add(record_length);
    if segment.end > circular_bound {
        return Err(ValidationError::new(
            format!("{segment_path}.end"),
            format!(
                "must not be greater than start + the circular record length {circular_bound} (at most one wrap)"
            ),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digest::{DigestValue, Digests, DIGEST_ALGORITHM_V1};
    use crate::extras::Extras;
    use crate::location::{Location, Segment};
    use crate::provenance::{Producer, ProducerKind, Provenance, SourceFormat};
    use crate::qualifier::Qualifier;
    use crate::record::{Kind, MoleculeType, Record, Topology};

    fn digest(fill: u8) -> DigestValue {
        DigestValue::from_sha256_bytes([fill; 32])
    }

    fn sample_record() -> Record {
        Record {
            schema: SCHEMA_ID.to_owned(),
            kind: Kind::Dna,
            molecule_type: Some(MoleculeType::DsDna),
            topology: Topology::Circular,
            name: "sample".to_owned(),
            description: Some("sample record".to_owned()),
            length: 12,
            sequence: "ATGAAACGCTAG".to_owned(),
            digests: Digests {
                algorithm: DIGEST_ALGORITHM_V1.to_owned(),
                sequence: digest(1),
                canonical: digest(2),
            },
            features: vec![Feature {
                key: "CDS".to_owned(),
                so_term: Some("SO:0000316".to_owned()),
                name: Some("orf".to_owned()),
                location: Location::span(1, 0, 12),
                qualifiers: vec![Qualifier::new("gene", "orf"), Qualifier::bare("pseudo")],
            }],
            provenance: Provenance {
                producer: Producer {
                    kind: ProducerKind::Parser,
                    name: "sequon-io".to_owned(),
                    version: "0.1.0".to_owned(),
                },
                source_format: SourceFormat::Genbank,
                source_digest: digest(3),
                source_bytes: 168,
                export_version: None,
            },
            extras: Extras::new(),
        }
    }

    fn assert_error(record: &Record, expected_path: &str) {
        match record.validate() {
            Ok(()) => panic!("expected validation to fail at {expected_path}"),
            Err(error) => assert_eq!(
                error.path(),
                expected_path,
                "unexpected validation error: {error}"
            ),
        }
    }

    /// Builds a valid record of the requested length and topology whose single
    /// feature location is `location`.
    fn record_of_length(length: u64, topology: Topology) -> Record {
        let mut record = sample_record();
        record.length = length;
        record.sequence = "A".repeat(length as usize);
        record.topology = topology;
        record.features[0].location = Location::span(1, 0, length);
        record
    }

    #[test]
    fn valid_record_passes() {
        assert!(sample_record().validate().is_ok());
    }

    #[test]
    fn schema_identifier_is_checked() {
        let mut record = sample_record();
        record.schema = "sequon/record/2".to_owned();
        assert_error(&record, "schema");
    }

    #[test]
    fn name_must_not_be_empty() {
        let mut record = sample_record();
        record.name.clear();
        assert_error(&record, "name");
    }

    #[test]
    fn length_must_be_positive_and_match_the_sequence() {
        let mut record = sample_record();
        record.length = 0;
        assert_error(&record, "length");

        let mut record = sample_record();
        record.length = 11;
        assert_error(&record, "length");
    }

    #[test]
    fn sequence_must_be_non_empty_uppercase_iupac() {
        let mut record = sample_record();
        record.sequence.clear();
        record.length = 0;
        assert_error(&record, "length");

        let mut record = sample_record();
        record.sequence = record.sequence.to_lowercase();
        assert_error(&record, "sequence");

        let mut record = sample_record();
        record.sequence = "ATGAAACGCUAG".to_owned();
        assert_error(&record, "sequence");

        let mut record = sample_record();
        record.sequence = "ATGAAACGCTA-".to_owned();
        assert_error(&record, "sequence");
    }

    #[test]
    fn digest_algorithm_is_checked() {
        let mut record = sample_record();
        record.digests.algorithm = "sequon-digest/2".to_owned();
        assert_error(&record, "digests.algorithm");
    }

    #[test]
    fn span_requires_exactly_one_segment() {
        let mut record = sample_record();
        record.features[0].location.segments = vec![Segment::new(0, 6), Segment::new(6, 12)];
        assert_error(&record, "features[0].location.segments");
    }

    #[test]
    fn join_requires_at_least_one_segment() {
        let mut record = sample_record();
        record.features[0].location.operator = OPERATOR_JOIN.to_owned();
        record.features[0].location.segments.clear();
        assert_error(&record, "features[0].location.segments");
    }

    #[test]
    fn operator_is_checked() {
        let mut record = sample_record();
        record.features[0].location.operator = "one-of".to_owned();
        assert_error(&record, "features[0].location.operator");
    }

    #[test]
    fn strand_range_is_checked() {
        let mut record = sample_record();
        record.features[0].location.strand = 2;
        assert_error(&record, "features[0].location.strand");
    }

    #[test]
    fn segment_coordinates_are_checked() {
        let mut record = sample_record();
        record.features[0].location.segments = vec![Segment::new(9, 4)];
        assert_error(&record, "features[0].location.segments[0].start");

        let mut record = sample_record();
        record.features[0].location.segments = vec![Segment::new(0, 13)];
        assert_error(&record, "features[0].location.segments[0].end");
    }

    #[test]
    fn circular_span_may_cross_the_origin_once() {
        let mut record = record_of_length(6000, Topology::Circular);

        // A single source range that crosses the origin is stored unwrapped.
        record.features[0].location = Location::span(1, 5899, 6100);
        assert!(
            record.validate().is_ok(),
            "a circular span with end > length must be valid: {:?}",
            record.validate().err()
        );

        // A full-circle span reaches exactly start + length.
        record.features[0].location = Location::span(1, 5899, 11899);
        assert!(
            record.validate().is_ok(),
            "a full-circle circular span must be valid: {:?}",
            record.validate().err()
        );

        // A between-positions site may sit at the last position of the circle.
        record.features[0].location = Location::span(0, 6000, 6000);
        assert!(
            record.validate().is_ok(),
            "a circular zero-length site at length must be valid: {:?}",
            record.validate().err()
        );
    }

    #[test]
    fn explicit_split_and_unwrapped_forms_are_both_valid_on_a_circular_record() {
        let mut record = record_of_length(6000, Topology::Circular);

        // GenBank join(5901..6000,1..100) crosses the origin explicitly and stays split.
        record.features[0].location =
            Location::join(1, vec![Segment::new(5900, 6000), Segment::new(0, 100)]);
        assert!(
            record.validate().is_ok(),
            "an explicit split must stay valid: {:?}",
            record.validate().err()
        );

        // The same bases expressed as one source range stay one unwrapped segment.
        record.features[0].location = Location::span(1, 5900, 6100);
        assert!(
            record.validate().is_ok(),
            "the unwrapped single-segment form must be valid: {:?}",
            record.validate().err()
        );
    }

    #[test]
    fn linear_span_must_stay_within_the_record() {
        let mut record = record_of_length(6000, Topology::Linear);
        record.features[0].location = Location::span(1, 5899, 6100);
        assert_error(&record, "features[0].location.segments[0].end");

        // A linear between-positions site also stays at or before the end.
        record.features[0].location = Location::span(0, 6001, 6001);
        assert_error(&record, "features[0].location.segments[0].end");
    }

    #[test]
    fn circular_span_rejects_more_than_one_wrap() {
        let mut record = record_of_length(6000, Topology::Circular);
        record.features[0].location = Location::span(1, 0, 12001);
        assert_error(&record, "features[0].location.segments[0].end");
        let error = record.validate().expect_err("must fail");
        assert!(
            error.message().contains("circular"),
            "the error must name the circular bound: {error}"
        );

        record.features[0].location = Location::span(1, 5899, 11900);
        assert_error(&record, "features[0].location.segments[0].end");
    }

    #[test]
    fn circular_span_rejects_start_at_or_after_the_length() {
        let mut record = record_of_length(6000, Topology::Circular);
        record.features[0].location = Location::span(1, 6000, 6100);
        assert_error(&record, "features[0].location.segments[0].start");
        let error = record.validate().expect_err("must fail");
        assert!(
            error.message().contains("circular"),
            "the error must name the circular bound: {error}"
        );

        record.features[0].location = Location::span(1, 6001, 6002);
        assert_error(&record, "features[0].location.segments[0].start");
    }

    #[test]
    fn circular_zero_length_site_must_not_exceed_the_length() {
        let mut record = record_of_length(6000, Topology::Circular);
        record.features[0].location = Location::span(0, 6001, 6001);
        assert_error(&record, "features[0].location.segments[0].end");
        let error = record.validate().expect_err("must fail");
        assert!(
            error.message().contains("circular"),
            "the error must name the circular bound: {error}"
        );
    }

    #[test]
    fn feature_and_qualifier_keys_are_checked() {
        let mut record = sample_record();
        record.features[0].key.clear();
        assert_error(&record, "features[0].key");

        let mut record = sample_record();
        record.features[0].qualifiers[0].key.clear();
        assert_error(&record, "features[0].qualifiers[0].key");
    }

    #[test]
    fn producer_fields_are_checked() {
        let mut record = sample_record();
        record.provenance.producer.name.clear();
        assert_error(&record, "provenance.producer.name");

        let mut record = sample_record();
        record.provenance.producer.version.clear();
        assert_error(&record, "provenance.producer.version");
    }

    #[test]
    fn error_display_contains_path_and_message() {
        let mut record = sample_record();
        record.name.clear();
        let error = record.validate().expect_err("must fail");
        assert!(error.to_string().starts_with("name: "));
        assert_eq!(error.path(), "name");
        assert!(!error.message().is_empty());
    }
}
