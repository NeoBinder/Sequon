//! Annotation-only transform contract tests.
//!
//! These tests exercise the pure sequon-algo surface only: no parser, HTTP,
//! database or platform identity. They prove atomic ordering, base-version
//! source-order identity, canonical validation, save-as-CDS semantics, unchanged
//! molecule identity and determinism, including a golden over the R11 6142 bp
//! synthetic fixture.

mod common;

use common::{make_record, qualifiers};
use sequon_algo::{
    add_feature, apply_annotation_operations, delete_feature, save_selection_as_cds,
    update_feature, AnnotationError, AnnotationOperation, AnnotationOutcome, FeatureInput,
    OperationIdentity, SaveAsCdsRequest, Selection, ANNOTATION_ALGORITHM_ID,
    ANNOTATION_ALGORITHM_VERSION, MAX_ANNOTATION_OPERATIONS, MAX_FEATURES_PER_RECORD,
    MAX_QUALIFIERS_PER_FEATURE, MAX_QUALIFIER_VALUE_BYTES, MAX_SEGMENTS_PER_FEATURE,
    SAVE_AS_CDS_KEY, SAVE_AS_CDS_SO_TERM,
};
use sequon_model::{
    DigestValue, Feature, JsonValue, Location, ProducerKind, Qualifier, Record, Segment, Topology,
};

/// 60 bp linear fixture sequence (12 bp motif repeated five times).
const SEQUENCE_60: &str = "ATGAAACGCTAGATGAAACGCTAGATGAAACGCTAGATGAAACGCTAGATGAAACGCTAG";

/// R11 synthetic 6142 bp golden shared with derived analysis (read-only here).
const R11_GOLDEN: &str = include_str!("../../../testdata/r11/r11-synthetic-analysis.json");

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn feature(key: &str, name: Option<&str>, location: Location, quals: Vec<Qualifier>) -> Feature {
    Feature {
        key: key.to_owned(),
        so_term: None,
        name: name.map(str::to_owned),
        location,
        qualifiers: quals,
    }
}

fn simple_feature(key: &str, start: u64, end: u64) -> Feature {
    feature(key, None, Location::span(1, start, end), Vec::new())
}

/// Base record with four source-order features:
/// 0 misc_feature [0,5), 1 CDS [5,20), 2 misc_feature [20,40), 3 promoter [40,60).
fn base_four_features() -> Record {
    make_record(
        SEQUENCE_60,
        Topology::Linear,
        vec![
            simple_feature("misc_feature", 0, 5),
            simple_feature("CDS", 5, 20),
            simple_feature("misc_feature", 20, 40),
            simple_feature("promoter", 40, 60),
        ],
    )
}

fn circular_base() -> Record {
    make_record(SEQUENCE_60, Topology::Circular, Vec::new())
}

fn assert_code<T: std::fmt::Debug>(result: Result<T, AnnotationError>, expected: &str) {
    match result {
        Ok(value) => panic!("expected {expected}, got {value:?}"),
        Err(error) => assert_eq!(error.code(), expected, "unexpected error: {error}"),
    }
}

/// Proves that a transform changed only annotations and the producer pin.
fn assert_molecule_unchanged(base: &Record, outcome: &AnnotationOutcome) {
    let result = &outcome.record;
    assert_eq!(result.sequence, base.sequence, "sequence must not change");
    assert_eq!(result.length, base.length, "length must not change");
    assert_eq!(result.topology, base.topology, "topology must not change");
    assert_eq!(
        result.digests, base.digests,
        "both molecule digests must not change"
    );
    assert_eq!(result.kind, base.kind);
    assert_eq!(result.molecule_type, base.molecule_type);
    assert_eq!(result.name, base.name);
    assert_eq!(result.description, base.description);
    assert_eq!(result.schema, base.schema);
    assert_eq!(result.extras, base.extras, "extras must be preserved");
    assert_eq!(
        result.provenance.source_format,
        base.provenance.source_format
    );
    assert_eq!(
        result.provenance.source_digest,
        base.provenance.source_digest
    );
    assert_eq!(result.provenance.source_bytes, base.provenance.source_bytes);
    assert_eq!(
        result.provenance.export_version,
        base.provenance.export_version
    );
    assert_eq!(
        result.provenance.producer.kind,
        ProducerKind::Algorithm,
        "the edited record records algorithm producer provenance"
    );
    assert_eq!(result.provenance.producer.name, "sequon-algo");
    assert_eq!(
        result.provenance.producer.version,
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(outcome.provenance.producer.name, "sequon-algo");
    assert_eq!(
        outcome.provenance.producer.version,
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(outcome.provenance.algorithm.id, ANNOTATION_ALGORITHM_ID);
    assert_eq!(
        outcome.provenance.algorithm.version,
        ANNOTATION_ALGORITHM_VERSION
    );
}

fn mixed_operations() -> Vec<AnnotationOperation> {
    vec![
        AnnotationOperation::DeleteFeature { feature_index: 0 },
        AnnotationOperation::UpdateFeature {
            feature_index: 2,
            feature: FeatureInput::new(
                "misc_feature",
                Location::span(-1, 20, 40),
                qualifiers(&[("note", "updated")]),
            ),
        },
        AnnotationOperation::AddFeature(FeatureInput::new(
            "misc_feature",
            Location::span(1, 0, 5),
            qualifiers(&[("note", "first-add")]),
        )),
        AnnotationOperation::SaveAsCds(SaveAsCdsRequest {
            selection: Selection::new(0, 3),
            strand: -1,
            name: Some("selection".to_owned()),
            codon_start: None,
            transl_table: None,
            qualifiers: Vec::new(),
        }),
        AnnotationOperation::AddFeature(FeatureInput::new(
            "misc_feature",
            Location::span(1, 5, 10),
            Vec::new(),
        )),
    ]
}

// ---------------------------------------------------------------------------
// Add / update / delete
// ---------------------------------------------------------------------------

#[test]
fn add_feature_appends_and_maps_identity() {
    let base = base_four_features();
    let snapshot = base.clone();
    let input = FeatureInput::new(
        "misc_feature",
        Location::span(-1, 10, 20),
        qualifiers(&[("note", "added")]),
    );

    let outcome = add_feature(&base, input).expect("a valid add succeeds");
    assert_eq!(base, snapshot, "the base Record must never be mutated");
    assert_eq!(outcome.record.features.len(), 5);
    assert_eq!(outcome.record.features[4].key, "misc_feature");
    assert_eq!(outcome.record.features[4].location.strand, -1);
    assert_eq!(
        outcome.record.features[4].qualifiers,
        qualifiers(&[("note", "added")])
    );
    assert_eq!(
        outcome.feature_identity_map.base_to_result,
        vec![Some(0), Some(1), Some(2), Some(3)]
    );
    assert_eq!(outcome.feature_identity_map.base_feature_count, 4);
    assert_eq!(outcome.feature_identity_map.result_feature_count, 5);
    assert_eq!(
        outcome.feature_identity_map.operation_to_result,
        vec![OperationIdentity {
            operation_index: 0,
            result_index: 4,
        }]
    );
    assert_molecule_unchanged(&base, &outcome);
}

#[test]
fn add_feature_accepts_a_zero_length_between_position_site() {
    let base = base_four_features();
    let input = FeatureInput::new("misc_feature", Location::span(1, 10, 10), Vec::new());
    let outcome = add_feature(&base, input).expect("a canonical zero-length site is legal");
    let segment = &outcome.record.features[4].location.segments[0];
    assert_eq!((segment.start, segment.end), (10, 10));
    assert_molecule_unchanged(&base, &outcome);
}

#[test]
fn update_feature_replaces_in_place() {
    let base = base_four_features();
    let replacement = FeatureInput::new(
        "CDS",
        Location::span(-1, 5, 20),
        qualifiers(&[("gene", "replaced")]),
    );
    let outcome = update_feature(&base, 1, replacement).expect("a valid update succeeds");

    assert_eq!(outcome.record.features.len(), 4);
    assert_eq!(outcome.record.features[1].key, "CDS");
    assert_eq!(outcome.record.features[1].location.strand, -1);
    assert_eq!(
        outcome.record.features[1].qualifiers,
        qualifiers(&[("gene", "replaced")])
    );
    assert_eq!(outcome.record.features[0], base.features[0]);
    assert_eq!(outcome.record.features[2], base.features[2]);
    assert_eq!(outcome.record.features[3], base.features[3]);
    assert_eq!(
        outcome.feature_identity_map.base_to_result,
        vec![Some(0), Some(1), Some(2), Some(3)]
    );
    assert!(outcome.feature_identity_map.operation_to_result.is_empty());
    assert_molecule_unchanged(&base, &outcome);
}

#[test]
fn delete_feature_reindexes_survivors() {
    let base = base_four_features();
    let outcome = delete_feature(&base, 1).expect("a valid delete succeeds");

    assert_eq!(outcome.record.features.len(), 3);
    assert_eq!(outcome.record.features[0], base.features[0]);
    assert_eq!(outcome.record.features[1], base.features[2]);
    assert_eq!(outcome.record.features[2], base.features[3]);
    assert_eq!(
        outcome.feature_identity_map.base_to_result,
        vec![Some(0), None, Some(1), Some(2)],
        "a deleted base feature maps to null; survivors are reindexed contiguously"
    );
    assert_eq!(outcome.feature_identity_map.result_feature_count, 3);
    assert!(outcome.feature_identity_map.operation_to_result.is_empty());
    assert_molecule_unchanged(&base, &outcome);
}

#[test]
fn deleting_every_feature_yields_an_empty_feature_list() {
    let base = make_record(
        SEQUENCE_60,
        Topology::Linear,
        vec![simple_feature("misc_feature", 0, 5)],
    );
    let outcome = delete_feature(&base, 0).expect("deleting the only feature is valid");
    assert!(outcome.record.features.is_empty());
    assert_eq!(outcome.feature_identity_map.base_to_result, vec![None]);
    assert_eq!(outcome.feature_identity_map.result_feature_count, 0);
    assert_molecule_unchanged(&base, &outcome);
}

#[test]
fn mixed_request_uses_base_identity_and_request_order() {
    let base = base_four_features();
    let outcome = apply_annotation_operations(&base, &mixed_operations())
        .expect("one atomic mixed request succeeds");

    assert_eq!(outcome.record.features.len(), 6);
    assert_eq!(
        outcome.record.features[0], base.features[1],
        "f1 survives first"
    );
    assert_eq!(
        outcome.record.features[1].location.strand, -1,
        "the update still refers to base index 2 after the base index 0 delete"
    );
    assert_eq!(outcome.record.features[2], base.features[3]);
    assert_eq!(
        outcome.record.features[3].key, "misc_feature",
        "adds and saves append in request order"
    );
    assert_eq!(outcome.record.features[4].key, SAVE_AS_CDS_KEY);
    assert_eq!(outcome.record.features[5].key, "misc_feature");
    assert_eq!(
        outcome.feature_identity_map.base_to_result,
        vec![None, Some(0), Some(1), Some(2)]
    );
    assert_eq!(
        outcome.feature_identity_map.operation_to_result,
        vec![
            OperationIdentity {
                operation_index: 2,
                result_index: 3,
            },
            OperationIdentity {
                operation_index: 3,
                result_index: 4,
            },
            OperationIdentity {
                operation_index: 4,
                result_index: 5,
            },
        ]
    );
    assert_molecule_unchanged(&base, &outcome);
}

// ---------------------------------------------------------------------------
// Identity resolution
// ---------------------------------------------------------------------------

#[test]
fn identity_out_of_range_is_rejected() {
    let base = base_four_features();
    let input = FeatureInput::new("misc_feature", Location::span(1, 0, 5), Vec::new());

    assert_code(
        update_feature(&base, 4, input.clone()),
        "ANNOTATION_IDENTITY_CONFLICT",
    );
    assert_code(delete_feature(&base, 4), "ANNOTATION_IDENTITY_CONFLICT");
    assert_code(
        delete_feature(&base, usize::MAX),
        "ANNOTATION_IDENTITY_CONFLICT",
    );
    let empty_base = make_record(SEQUENCE_60, Topology::Linear, Vec::new());
    assert_code(
        delete_feature(&empty_base, 0),
        "ANNOTATION_IDENTITY_CONFLICT",
    );
}

#[test]
fn duplicate_targets_are_rejected() {
    let base = base_four_features();
    let replacement = FeatureInput::new("misc_feature", Location::span(1, 0, 5), Vec::new());
    let update = AnnotationOperation::UpdateFeature {
        feature_index: 1,
        feature: replacement.clone(),
    };
    let delete = AnnotationOperation::DeleteFeature { feature_index: 1 };

    let cases: [(&str, Vec<AnnotationOperation>); 3] = [
        ("update then update", vec![update.clone(), update.clone()]),
        ("update then delete", vec![update.clone(), delete.clone()]),
        ("delete then update", vec![delete.clone(), update]),
    ];
    for (label, operations) in cases {
        match apply_annotation_operations(&base, &operations) {
            Ok(_) => panic!("{label} must be rejected"),
            Err(error) => {
                assert_eq!(error.code(), "ANNOTATION_IDENTITY_CONFLICT", "{label}");
                match error {
                    AnnotationError::IdentityConflict {
                        operation_index,
                        feature_index,
                    } => {
                        assert_eq!(operation_index, 1, "{label}");
                        assert_eq!(feature_index, 1, "{label}");
                    }
                    other => panic!("{label}: unexpected error {other}"),
                }
            }
        }
    }
    let double_delete = vec![delete.clone(), delete];
    assert_code(
        apply_annotation_operations(&base, &double_delete),
        "ANNOTATION_IDENTITY_CONFLICT",
    );
}

// ---------------------------------------------------------------------------
// Feature / location validation
// ---------------------------------------------------------------------------

#[test]
fn invalid_feature_inputs_are_rejected() {
    let base = base_four_features();
    let circular = circular_base();

    let cases: Vec<(&str, Record, FeatureInput, &str)> = vec![
        (
            "empty key",
            base.clone(),
            FeatureInput::new("", Location::span(1, 0, 5), Vec::new()),
            "ANNOTATION_FEATURE_INVALID",
        ),
        (
            "over-long key",
            base.clone(),
            FeatureInput::new("k".repeat(256), Location::span(1, 0, 5), Vec::new()),
            "ANNOTATION_FEATURE_INVALID",
        ),
        (
            "empty qualifier key",
            base.clone(),
            FeatureInput::new(
                "misc_feature",
                Location::span(1, 0, 5),
                vec![Qualifier::new("", "value")],
            ),
            "ANNOTATION_FEATURE_INVALID",
        ),
        (
            "over-long qualifier key",
            base.clone(),
            FeatureInput::new(
                "misc_feature",
                Location::span(1, 0, 5),
                vec![Qualifier::new("q".repeat(256), "value")],
            ),
            "ANNOTATION_FEATURE_INVALID",
        ),
        (
            "control character in qualifier key",
            base.clone(),
            FeatureInput::new(
                "misc_feature",
                Location::span(1, 0, 5),
                vec![Qualifier::new("no\u{0007}pe", "value")],
            ),
            "ANNOTATION_FEATURE_INVALID",
        ),
        (
            "unknown operator",
            base.clone(),
            FeatureInput::new(
                "misc_feature",
                Location {
                    operator: "translation".to_owned(),
                    strand: 1,
                    segments: vec![Segment::new(0, 5)],
                },
                Vec::new(),
            ),
            "ANNOTATION_FEATURE_INVALID",
        ),
        (
            "span with two segments",
            base.clone(),
            FeatureInput::new(
                "misc_feature",
                Location {
                    operator: "span".to_owned(),
                    strand: 1,
                    segments: vec![Segment::new(0, 5), Segment::new(5, 10)],
                },
                Vec::new(),
            ),
            "ANNOTATION_FEATURE_INVALID",
        ),
        (
            "join without segments",
            base.clone(),
            FeatureInput::new(
                "misc_feature",
                Location {
                    operator: "join".to_owned(),
                    strand: 1,
                    segments: Vec::new(),
                },
                Vec::new(),
            ),
            "ANNOTATION_FEATURE_INVALID",
        ),
        (
            "strand outside -1,0,1",
            base.clone(),
            FeatureInput::new("misc_feature", Location::span(2, 0, 5), Vec::new()),
            "ANNOTATION_FEATURE_INVALID",
        ),
        (
            "linear end beyond the record",
            base.clone(),
            FeatureInput::new("misc_feature", Location::span(1, 0, 61), Vec::new()),
            "ANNOTATION_FEATURE_INVALID",
        ),
        (
            "circular spanning start not below length",
            circular.clone(),
            FeatureInput::new("misc_feature", Location::span(1, 60, 62), Vec::new()),
            "ANNOTATION_FEATURE_INVALID",
        ),
        (
            "circular more than one wrap",
            circular,
            FeatureInput::new("misc_feature", Location::span(1, 59, 121), Vec::new()),
            "ANNOTATION_FEATURE_INVALID",
        ),
    ];

    for (label, record, input, code) in cases {
        match add_feature(&record, input) {
            Ok(_) => panic!("{label} must be rejected"),
            Err(error) => assert_eq!(error.code(), code, "{label}: {error}"),
        }
    }
}

#[test]
fn join_and_order_locations_are_preserved_verbatim() {
    let base = circular_base();
    let join = Location::join(-1, vec![Segment::new(50, 60), Segment::new(0, 10)]);
    let order = Location::order(1, vec![Segment::new(5, 10), Segment::new(20, 25)]);
    let outcome = apply_annotation_operations(
        &base,
        &[
            AnnotationOperation::AddFeature(FeatureInput::new(
                "misc_feature",
                join.clone(),
                Vec::new(),
            )),
            AnnotationOperation::AddFeature(FeatureInput::new(
                "misc_feature",
                order.clone(),
                Vec::new(),
            )),
        ],
    )
    .expect("join and order additions succeed");
    assert_eq!(outcome.record.features[0].location, join);
    assert_eq!(outcome.record.features[1].location, order);
    assert_molecule_unchanged(&base, &outcome);
}

#[test]
fn partial_boundaries_survive_add_and_update() {
    let base = base_four_features();
    let location = Location {
        operator: "span".to_owned(),
        strand: 1,
        segments: vec![Segment::with_boundaries(0, 15, true, true)],
    };
    let outcome = add_feature(
        &base,
        FeatureInput::new("misc_feature", location.clone(), Vec::new()),
    )
    .expect("partial boundaries are canonical");
    let segment = &outcome.record.features[4].location.segments[0];
    assert!(segment.start_partial);
    assert!(segment.end_partial);
    assert_eq!((segment.start, segment.end), (0, 15));
    assert_molecule_unchanged(&base, &outcome);
}

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

#[test]
fn limit_violations_are_rejected() {
    let base = circular_base();

    let too_many_operations: Vec<AnnotationOperation> = (0..MAX_ANNOTATION_OPERATIONS + 1)
        .map(|_| {
            AnnotationOperation::AddFeature(FeatureInput::new(
                "misc_feature",
                Location::span(1, 0, 5),
                Vec::new(),
            ))
        })
        .collect();
    assert_code(
        apply_annotation_operations(&base, &too_many_operations),
        "ANNOTATION_LIMIT_EXCEEDED",
    );

    let many_segments = Location::join(
        1,
        (0..MAX_SEGMENTS_PER_FEATURE + 1)
            .map(|_| Segment::new(0, 1))
            .collect(),
    );
    assert_code(
        add_feature(
            &base,
            FeatureInput::new("misc_feature", many_segments, Vec::new()),
        ),
        "ANNOTATION_LIMIT_EXCEEDED",
    );

    let many_qualifiers: Vec<Qualifier> = (0..MAX_QUALIFIERS_PER_FEATURE + 1)
        .map(|index| Qualifier::new(format!("note{index}"), "value"))
        .collect();
    assert_code(
        add_feature(
            &base,
            FeatureInput::new("misc_feature", Location::span(1, 0, 5), many_qualifiers),
        ),
        "ANNOTATION_LIMIT_EXCEEDED",
    );

    let oversized_value = "v".repeat(MAX_QUALIFIER_VALUE_BYTES + 1);
    assert_code(
        add_feature(
            &base,
            FeatureInput::new(
                "misc_feature",
                Location::span(1, 0, 5),
                vec![Qualifier::new("note", oversized_value)],
            ),
        ),
        "ANNOTATION_LIMIT_EXCEEDED",
    );

    // The result feature count is limited: a base at the limit cannot grow.
    let full_base = make_record(
        SEQUENCE_60,
        Topology::Linear,
        (0..MAX_FEATURES_PER_RECORD)
            .map(|_| simple_feature("misc_feature", 0, 5))
            .collect(),
    );
    assert_code(
        add_feature(
            &full_base,
            FeatureInput::new("misc_feature", Location::span(1, 0, 5), Vec::new()),
        ),
        "ANNOTATION_LIMIT_EXCEEDED",
    );
}

// ---------------------------------------------------------------------------
// save-selection-as-CDS
// ---------------------------------------------------------------------------

#[test]
fn save_as_cds_builds_one_span_and_writes_protected_qualifiers() {
    let base = base_four_features();
    let outcome = save_selection_as_cds(
        &base,
        SaveAsCdsRequest {
            selection: Selection::new(0, 9),
            strand: 1,
            name: Some("selection-cds".to_owned()),
            codon_start: Some(2),
            transl_table: Some(11),
            qualifiers: qualifiers(&[("gene", "sel"), ("note", "from selection")]),
        },
    )
    .expect("a triplet selection becomes a CDS");

    let cds = &outcome.record.features[4];
    assert_eq!(cds.key, SAVE_AS_CDS_KEY);
    assert_eq!(cds.so_term.as_deref(), Some(SAVE_AS_CDS_SO_TERM));
    assert_eq!(cds.name.as_deref(), Some("selection-cds"));
    assert_eq!(cds.location.operator, "span");
    assert_eq!(cds.location.strand, 1);
    assert_eq!(cds.location.segments.len(), 1);
    assert_eq!(
        (cds.location.segments[0].start, cds.location.segments[0].end),
        (0, 9)
    );
    assert_eq!(
        cds.qualifiers,
        qualifiers(&[
            ("gene", "sel"),
            ("note", "from selection"),
            ("codon_start", "2"),
            ("transl_table", "11"),
        ]),
        "caller qualifiers keep their order and the written pair comes last once"
    );
    assert_molecule_unchanged(&base, &outcome);

    // Absent codon_start/transl_table default to 1 and 1.
    let defaults = save_selection_as_cds(
        &base,
        SaveAsCdsRequest {
            selection: Selection::new(3, 6),
            strand: -1,
            name: None,
            codon_start: None,
            transl_table: None,
            qualifiers: Vec::new(),
        },
    )
    .expect("defaults are written");
    assert_eq!(
        defaults.record.features[4].qualifiers,
        qualifiers(&[("codon_start", "1"), ("transl_table", "1")])
    );
    assert_eq!(defaults.record.features[4].location.strand, -1);
}

#[test]
fn save_as_cds_supports_an_unwrapped_circular_origin_crossing_span() {
    let base = circular_base();
    let outcome = save_selection_as_cds(
        &base,
        SaveAsCdsRequest {
            selection: Selection::new(57, 66),
            strand: 1,
            name: None,
            codon_start: None,
            transl_table: None,
            qualifiers: Vec::new(),
        },
    )
    .expect("a one-wrap circular selection is a canonical span");

    let cds = &outcome.record.features[0];
    let segment = &cds.location.segments[0];
    assert_eq!((segment.start, segment.end), (57, 66));
    assert!(
        cds.location.is_wrapped(base.length),
        "the CDS keeps the unwrapped origin-crossing span"
    );
    assert_molecule_unchanged(&base, &outcome);
}

#[test]
fn save_as_cds_rejects_invalid_selection_and_fields() {
    let linear = base_four_features();
    let circular = circular_base();

    let request = |selection: Selection, strand: i8| SaveAsCdsRequest {
        selection,
        strand,
        name: None,
        codon_start: None,
        transl_table: None,
        qualifiers: Vec::new(),
    };

    assert_code(
        save_selection_as_cds(&linear, request(Selection::new(0, 9), 0)),
        "ANNOTATION_FEATURE_INVALID",
    );
    assert_code(
        save_selection_as_cds(&linear, request(Selection::new(5, 5), 1)),
        "ANNOTATION_FEATURE_INVALID",
    );
    assert_code(
        save_selection_as_cds(&linear, request(Selection::new(0, 61), 1)),
        "ANNOTATION_FEATURE_INVALID",
    );
    assert_code(
        save_selection_as_cds(&circular, request(Selection::new(59, 121), 1)),
        "ANNOTATION_FEATURE_INVALID",
    );
    assert_code(
        save_selection_as_cds(&linear, request(Selection::new(0, 10), 1)),
        "ANNOTATION_CDS_LENGTH_INVALID",
    );
    assert_code(
        save_selection_as_cds(
            &linear,
            SaveAsCdsRequest {
                codon_start: Some(4),
                ..request(Selection::new(0, 9), 1)
            },
        ),
        "ANNOTATION_FEATURE_INVALID",
    );
    assert_code(
        save_selection_as_cds(
            &linear,
            SaveAsCdsRequest {
                transl_table: Some(2),
                ..request(Selection::new(0, 9), 1)
            },
        ),
        "ANNOTATION_FEATURE_INVALID",
    );

    for key in ["codon_start", "transl_table", "translation"] {
        assert_code(
            save_selection_as_cds(
                &linear,
                SaveAsCdsRequest {
                    qualifiers: vec![Qualifier::new(key, "1")],
                    ..request(Selection::new(0, 9), 1)
                },
            ),
            "ANNOTATION_QUALIFIER_UNSUPPORTED",
        );
    }

    let too_many: Vec<Qualifier> = (0..MAX_QUALIFIERS_PER_FEATURE - 1)
        .map(|index| Qualifier::new(format!("note{index}"), "value"))
        .collect();
    assert_code(
        save_selection_as_cds(
            &linear,
            SaveAsCdsRequest {
                qualifiers: too_many,
                ..request(Selection::new(0, 9), 1)
            },
        ),
        "ANNOTATION_LIMIT_EXCEEDED",
    );
}

// ---------------------------------------------------------------------------
// Protected /translation qualifier
// ---------------------------------------------------------------------------

/// Base with one CDS carrying the ordered translation entries [translation=MKV].
fn base_with_translation() -> Record {
    make_record(
        SEQUENCE_60,
        Topology::Linear,
        vec![feature(
            "CDS",
            Some("source"),
            Location::span(1, 0, 30),
            qualifiers(&[("gene", "g"), ("translation", "MKV")]),
        )],
    )
}

#[test]
fn update_must_preserve_base_translation_entries() {
    let base = base_with_translation();

    let identical = FeatureInput::new(
        "CDS",
        Location::span(1, 0, 30),
        qualifiers(&[("gene", "g2"), ("translation", "MKV")]),
    );
    let outcome = update_feature(&base, 0, identical).expect("identical translation is accepted");
    assert_eq!(
        outcome.record.features[0].qualifiers,
        qualifiers(&[("gene", "g2"), ("translation", "MKV")])
    );
    assert_molecule_unchanged(&base, &outcome);

    let changed = FeatureInput::new(
        "CDS",
        Location::span(1, 0, 30),
        qualifiers(&[("translation", "MKW")]),
    );
    assert_code(
        update_feature(&base, 0, changed),
        "ANNOTATION_QUALIFIER_UNSUPPORTED",
    );

    let omitted = FeatureInput::new(
        "CDS",
        Location::span(1, 0, 30),
        qualifiers(&[("gene", "g")]),
    );
    assert_code(
        update_feature(&base, 0, omitted),
        "ANNOTATION_QUALIFIER_UNSUPPORTED",
    );

    let plain_base = base_four_features();
    let introduced = FeatureInput::new(
        "CDS",
        Location::span(1, 5, 20),
        qualifiers(&[("translation", "MKV")]),
    );
    assert_code(
        update_feature(&plain_base, 1, introduced),
        "ANNOTATION_QUALIFIER_UNSUPPORTED",
    );

    let added = FeatureInput::new(
        "CDS",
        Location::span(1, 0, 30),
        qualifiers(&[("translation", "MKV")]),
    );
    assert_code(
        add_feature(&base, added),
        "ANNOTATION_QUALIFIER_UNSUPPORTED",
    );
}

#[test]
fn duplicate_translation_entries_must_stay_identical_and_ordered() {
    let base = make_record(
        SEQUENCE_60,
        Topology::Linear,
        vec![feature(
            "CDS",
            None,
            Location::span(1, 0, 30),
            vec![
                Qualifier::new("translation", "MKV"),
                Qualifier::new("translation", "MKW"),
            ],
        )],
    );
    let identical = FeatureInput::new(
        "CDS",
        Location::span(1, 0, 30),
        vec![
            Qualifier::new("translation", "MKV"),
            Qualifier::new("translation", "MKW"),
        ],
    );
    update_feature(&base, 0, identical).expect("identical ordered entries are accepted");

    let reversed = FeatureInput::new(
        "CDS",
        Location::span(1, 0, 30),
        vec![
            Qualifier::new("translation", "MKW"),
            Qualifier::new("translation", "MKV"),
        ],
    );
    assert_code(
        update_feature(&base, 0, reversed),
        "ANNOTATION_QUALIFIER_UNSUPPORTED",
    );
}

// ---------------------------------------------------------------------------
// Invariance, provenance, determinism and errors
// ---------------------------------------------------------------------------

#[test]
fn annotation_never_edits_bases_and_updates_producer_provenance() {
    let mut base = base_four_features();
    base.extras
        .insert("test-namespace", JsonValue::String("kept".to_owned()));
    base.provenance.export_version = Some(15);

    let outcome = apply_annotation_operations(&base, &mixed_operations())
        .expect("a mixed annotation request succeeds");
    assert_molecule_unchanged(&base, &outcome);
    assert_eq!(
        outcome.record.extras.get("test-namespace"),
        Some(&JsonValue::String("kept".to_owned())),
        "extras survive the transform"
    );
    assert_eq!(outcome.record.provenance.export_version, Some(15));
    assert_ne!(
        outcome.record, base,
        "the document identity changes because features and producer changed"
    );
}

#[test]
fn no_import_diagnostics_are_inherited() {
    // The canonical Record has no diagnostics/report field at all: parser
    // warnings, dropped entries and preserved blocks live in the separate
    // sequon/import-report/1 document and are never part of a Record. The
    // transform takes no report input, so a new annotation version has no
    // inherited parser report; only original source lineage fields are kept.
    let mut base = base_four_features();
    base.extras
        .insert("parser-namespace", JsonValue::String("kept".to_owned()));
    let outcome = update_feature(
        &base,
        0,
        FeatureInput::new("misc_feature", Location::span(1, 0, 5), Vec::new()),
    )
    .expect("update succeeds");
    assert_eq!(
        outcome.record.provenance.source_digest, base.provenance.source_digest,
        "source lineage is retained"
    );
    assert_eq!(
        outcome.record.provenance.source_bytes,
        base.provenance.source_bytes
    );
    assert_eq!(
        outcome.record.provenance.source_format,
        base.provenance.source_format
    );
    assert_eq!(
        outcome.record.extras.get("parser-namespace"),
        Some(&JsonValue::String("kept".to_owned()))
    );
    assert_eq!(
        outcome.record.provenance.producer.kind,
        ProducerKind::Algorithm
    );
    assert_molecule_unchanged(&base, &outcome);
}

#[test]
fn repeated_application_is_deterministic() {
    let base = base_four_features();
    let operations = mixed_operations();
    let first = apply_annotation_operations(&base, &operations).expect("first transform");
    let second = apply_annotation_operations(&base, &operations).expect("second transform");
    assert_eq!(
        first, second,
        "same base and request produce the same outcome"
    );
    assert_eq!(first.record, second.record);
    assert_eq!(first.feature_identity_map, second.feature_identity_map);
    assert_eq!(first.provenance, second.provenance);
}

#[test]
fn empty_requests_and_invalid_bases_are_rejected() {
    let base = base_four_features();
    assert_code(
        apply_annotation_operations(&base, &[]),
        "ANNOTATION_FEATURE_INVALID",
    );

    let mut bad_digest = base.clone();
    bad_digest.digests.sequence =
        DigestValue::parse(&format!("sha256:{}", "0".repeat(64))).expect("valid digest syntax");
    assert_code(
        add_feature(
            &bad_digest,
            FeatureInput::new("misc_feature", Location::span(1, 0, 5), Vec::new()),
        ),
        "DIGEST_MISMATCH",
    );

    let mut bad_record = base.clone();
    bad_record.length = 61;
    assert_code(
        add_feature(
            &bad_record,
            FeatureInput::new("misc_feature", Location::span(1, 0, 5), Vec::new()),
        ),
        "RECORD_INVALID",
    );
}

#[test]
fn error_messages_do_not_echo_sequence_or_qualifier_values() {
    let base = base_with_translation();
    let changed = FeatureInput::new(
        "CDS",
        Location::span(1, 0, 30),
        qualifiers(&[("translation", "SECRETVALUE")]),
    );
    let error = update_feature(&base, 0, changed).expect_err("changed translation is rejected");
    let message = error.to_string();
    assert!(!message.contains("SECRETVALUE"), "{message}");
    assert!(!message.contains("MKV"), "{message}");

    let out_of_range = FeatureInput::new("misc_feature", Location::span(1, 0, 999), Vec::new());
    let error = add_feature(&base, out_of_range).expect_err("out-of-range location is rejected");
    let message = error.to_string();
    assert!(!message.contains(SEQUENCE_60), "{message}");
    assert!(!message.contains("ATGAAACGC"), "{message}");
}

// ---------------------------------------------------------------------------
// R11 6142 bp golden
// ---------------------------------------------------------------------------

fn golden_location(value: &serde_json::Value) -> Location {
    Location {
        operator: value["operator"].as_str().expect("operator").to_owned(),
        strand: value["strand"].as_i64().expect("strand") as i8,
        segments: value["segments"]
            .as_array()
            .expect("segments")
            .iter()
            .map(|segment| {
                Segment::new(
                    segment["start"].as_u64().expect("start"),
                    segment["end"].as_u64().expect("end"),
                )
            })
            .collect(),
    }
}

fn golden_qualifiers(value: &serde_json::Value) -> Vec<Qualifier> {
    value
        .as_array()
        .expect("qualifier array")
        .iter()
        .map(|qualifier| {
            Qualifier::new(
                qualifier["key"].as_str().expect("qualifier key"),
                qualifier["value"].as_str().expect("qualifier value"),
            )
        })
        .collect()
}

fn r11_golden_base() -> Record {
    let root: serde_json::Value = serde_json::from_str(R11_GOLDEN).expect("golden JSON");
    let sequence = root["record"]["sequence"].as_str().expect("sequence");
    assert_eq!(sequence.len(), 6142);
    let features: Vec<Feature> = root["features"]
        .as_array()
        .expect("features")
        .iter()
        .map(|value| Feature {
            key: value["key"].as_str().expect("key").to_owned(),
            so_term: Some(SAVE_AS_CDS_SO_TERM.to_owned()),
            name: value["name"].as_str().map(str::to_owned),
            location: golden_location(&value["location"]),
            qualifiers: golden_qualifiers(&value["qualifiers"]),
        })
        .collect();
    make_record(sequence, Topology::Circular, features)
}

#[test]
fn r11_6142bp_annotation_golden() {
    let base = r11_golden_base();
    assert_eq!(base.length, 6142);
    assert_eq!(base.features.len(), 2);

    let operations = vec![
        AnnotationOperation::UpdateFeature {
            feature_index: 0,
            feature: FeatureInput {
                key: "CDS".to_owned(),
                so_term: Some(SAVE_AS_CDS_SO_TERM.to_owned()),
                name: Some("r11-812aa-updated".to_owned()),
                location: base.features[0].location.clone(),
                qualifiers: qualifiers(&[
                    ("gene", "r11-edit"),
                    ("codon_start", "1"),
                    ("transl_table", "1"),
                ]),
            },
        },
        AnnotationOperation::DeleteFeature { feature_index: 1 },
        AnnotationOperation::AddFeature(FeatureInput::new(
            "misc_feature",
            Location::span(1, 6100, 6152),
            qualifiers(&[("note", "crosses the origin")]),
        )),
        AnnotationOperation::SaveAsCds(SaveAsCdsRequest {
            selection: Selection::new(0, 3),
            strand: 1,
            name: Some("start-codon".to_owned()),
            codon_start: None,
            transl_table: None,
            qualifiers: qualifiers(&[("gene", "start")]),
        }),
    ];

    let outcome = apply_annotation_operations(&base, &operations).expect("R11 golden edit");
    let result = &outcome.record;
    assert_eq!(result.length, 6142);
    assert_eq!(result.topology, Topology::Circular);
    assert_eq!(result.features.len(), 3);
    assert_eq!(
        result.features[0].name.as_deref(),
        Some("r11-812aa-updated")
    );
    assert_eq!(result.features[1].key, "misc_feature");
    assert_eq!(
        (
            result.features[1].location.segments[0].start,
            result.features[1].location.segments[0].end
        ),
        (6100, 6152),
        "the origin-crossing span stays unwrapped in the canonical document"
    );
    assert!(result.features[1].location.is_wrapped(6142));
    assert_eq!(result.features[2].key, SAVE_AS_CDS_KEY);
    assert_eq!(
        result.features[2].qualifiers,
        qualifiers(&[
            ("gene", "start"),
            ("codon_start", "1"),
            ("transl_table", "1"),
        ])
    );
    assert_eq!(
        outcome.feature_identity_map.base_to_result,
        vec![Some(0), None]
    );
    assert_eq!(
        outcome.feature_identity_map.operation_to_result,
        vec![
            OperationIdentity {
                operation_index: 2,
                result_index: 1,
            },
            OperationIdentity {
                operation_index: 3,
                result_index: 2,
            },
        ]
    );
    assert_molecule_unchanged(&base, &outcome);

    let repeated = apply_annotation_operations(&base, &operations).expect("deterministic rerun");
    assert_eq!(outcome, repeated);
}
