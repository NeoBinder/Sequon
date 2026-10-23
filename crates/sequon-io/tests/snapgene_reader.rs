#![cfg(feature = "snapgene")]

//! Integration tests for the SnapGene .dna reader.
//!
//! Containers are built in-test from self-authored sequences; the local corpus
//! is never copied. The corpus acceptance test lives in
//! tests/snapgene_corpus.rs and runs only when the gitignored file exists.

use sequon_io::api::{Format, ImportOptions};
use sequon_io::error::ImportError;
use sequon_io::report::Severity;
use sequon_io::report::SourcePosition;

fn packet(packet_type: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(5 + payload.len());
    bytes.push(packet_type);
    bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

fn container(sequence_type: u16, export_version: u16, packets: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut cookie = Vec::new();
    cookie.extend_from_slice(b"SnapGene");
    cookie.extend_from_slice(&sequence_type.to_be_bytes());
    cookie.extend_from_slice(&export_version.to_be_bytes());
    cookie.extend_from_slice(&19_u16.to_be_bytes());
    let mut bytes = packet(0x09, &cookie);
    for (packet_type, payload) in packets {
        bytes.extend_from_slice(&packet(*packet_type, payload));
    }
    bytes
}

fn dna(flags: u8, bases: &str) -> Vec<u8> {
    let mut payload = vec![flags];
    payload.extend_from_slice(bases.as_bytes());
    payload
}

fn dna_2bit(flags: u8, bases: &str) -> Vec<u8> {
    let mut packed = Vec::new();
    let mut accumulator = 0_u8;
    let mut filled = 0_u8;
    for base in bases.chars() {
        let value = match base {
            'G' => 0_u8,
            'A' => 1,
            'T' => 2,
            'C' => 3,
            other => panic!("2-bit test builder only supports GATC, found {other}"),
        };
        accumulator = (accumulator << 2) | value;
        filled += 1;
        if filled == 4 {
            packed.push(accumulator);
            accumulator = 0;
            filled = 0;
        }
    }
    if filled > 0 {
        packed.push(accumulator << (2 * (4 - filled)));
    }
    let mut payload = vec![flags];
    payload.extend_from_slice(&(bases.len() as u32).to_be_bytes());
    payload.extend_from_slice(&packed);
    payload
}

fn features_xml(inner: &str) -> String {
    format!("<?xml version=\"1.0\"?><Features nextValidID=\"1\">{inner}</Features>")
}

fn notes_xml(inner: &str) -> String {
    format!("<Notes>{inner}</Notes>")
}

fn properties_xml(inner: &str) -> String {
    format!("<AdditionalSequenceProperties>{inner}</AdditionalSequenceProperties>")
}

fn empty_primers_xml() -> String {
    "<?xml version=\"1.0\"?><Primers nextValidID=\"0\"><HybridizationParams minMeltingTemperature=\"40\"/></Primers>".to_string()
}

fn default_options() -> ImportOptions {
    ImportOptions {
        format: Some(Format::Snapgene),
        ..ImportOptions::default()
    }
}

fn import_all(bytes: &[u8]) -> sequon_io::api::ImportOutcome {
    sequon_io::import(bytes, &default_options()).expect("import must succeed")
}

fn import_error(bytes: &[u8]) -> ImportError {
    sequon_io::import(bytes, &default_options()).expect_err("import must fail")
}

fn base_packets(flags: u8, bases: &str) -> Vec<(u8, Vec<u8>)> {
    vec![
        (0, dna(flags, bases)),
        (
            6,
            notes_xml("<CustomMapLabel>synthetic plasmid</CustomMapLabel><UseCustomMapLabel>1</UseCustomMapLabel>")
                .into_bytes(),
        ),
    ]
}

#[test]
fn minimal_container_produces_a_valid_record() {
    let bytes = container(
        1,
        15,
        &base_packets(0x01, "ATGCGTACGTAGCTAGCTAGCATCGATCGATCGA"),
    );
    let outcome = import_all(&bytes);
    assert_eq!(outcome.records.len(), 1);
    let record = &outcome.records[0];
    assert_eq!(record.name, "synthetic plasmid");
    assert_eq!(record.topology, sequon_model::Topology::Circular);
    assert_eq!(record.kind, sequon_model::Kind::Dna);
    assert_eq!(record.molecule_type, None);
    assert_eq!(record.length, 34);
    assert_eq!(record.sequence, "ATGCGTACGTAGCTAGCTAGCATCGATCGATCGA");
    assert_eq!(record.provenance.export_version, Some(15));
    assert_eq!(record.provenance.source_bytes, bytes.len() as u64);
    record.validate().expect("record must validate");
    sequon_algo::verify_digests(record).expect("digests must verify");
    assert_eq!(outcome.report.source_bytes, bytes.len() as u64);
    assert_eq!(outcome.report.export_version, Some(15));
    assert_eq!(outcome.report.parser.name, "sequon-io");
    assert_eq!(outcome.report.codes_version, "2026-10-21/1");
    assert!(outcome.report.dropped.is_empty());
    assert!(outcome.report.preserved.is_empty());
}

#[test]
fn flags_zero_is_linear_and_unknown_flag_bits_are_reported() {
    let bytes = container(1, 15, &base_packets(0x00, "ACGTACGTACGT"));
    let outcome = import_all(&bytes);
    assert_eq!(outcome.records[0].topology, sequon_model::Topology::Linear);
    assert!(outcome.report.diagnostics.is_empty());

    let bytes = container(1, 15, &base_packets(0x21, "ACGTACGTACGT"));
    let outcome = import_all(&bytes);
    assert_eq!(
        outcome.records[0].topology,
        sequon_model::Topology::Circular
    );
    let diagnostic = &outcome.report.diagnostics[0];
    assert_eq!(diagnostic.code, "DNA_TOPOLOGY_FLAG_UNKNOWN");
    assert_eq!(diagnostic.severity, Severity::Info);
}

#[test]
fn lowercase_and_whitespace_are_normalized_with_an_info() {
    let bytes = container(1, 15, &base_packets(0x00, "acgt ACGT\nA C G T"));
    let outcome = import_all(&bytes);
    assert_eq!(outcome.records[0].sequence, "ACGTACGTACGT");
    assert!(outcome
        .report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "DNA_SEQUENCE_NORMALIZED"));
}

#[test]
fn digits_are_rejected() {
    let bytes = container(1, 15, &base_packets(0x00, "ACGT1ACGT"));
    let error = import_error(&bytes);
    assert_eq!(error.code(), "DNA_SEQUENCE_INVALID_CHAR");
}

#[test]
fn empty_sequence_is_rejected() {
    let bytes = container(1, 15, &[(0, dna(0x00, ""))]);
    let error = import_error(&bytes);
    assert_eq!(error.code(), "DNA_SEQUENCE_EMPTY");
}

#[test]
fn container_level_errors_are_typed() {
    let mut bad = container(1, 15, &base_packets(0x00, "ACGT"));
    bad[0] = 0x08;
    assert_eq!(import_error(&bad).code(), "DNA_BAD_MAGIC");

    let good = container(1, 15, &base_packets(0x00, "ACGTACGT"));
    let error = import_error(&good[..good.len() - 2]);
    assert_eq!(error.code(), "DNA_TRUNCATED_PACKET");

    let metadata_only = container(
        1,
        15,
        &[(
            6,
            notes_xml("<CustomMapLabel>x</CustomMapLabel>").into_bytes(),
        )],
    );
    assert_eq!(import_error(&metadata_only).code(), "DNA_NO_DNA_PACKET");
}

#[test]
fn two_dna_packets_of_any_kind_are_rejected() {
    let bytes = container(1, 15, &[(0, dna(0x00, "ACGT")), (0, dna(0x00, "TGCA"))]);
    assert_eq!(import_error(&bytes).code(), "DNA_DUPLICATE_DNA_PACKET");

    let bytes = container(
        1,
        15,
        &[(0, dna(0x00, "ACGT")), (1, dna_2bit(0x00, "TGCA"))],
    );
    assert_eq!(import_error(&bytes).code(), "DNA_DUPLICATE_DNA_PACKET");
}

#[test]
fn only_sequence_type_one_is_dna() {
    for sequence_type in [0_u16, 2, 7] {
        let bytes = container(sequence_type, 15, &[(0, dna(0x00, "ACGT"))]);
        assert_eq!(import_error(&bytes).code(), "DNA_UNKNOWN_SEQ_TYPE");
    }
}

#[test]
fn two_bit_hypothesis_decodes_the_same_sequence_as_type_zero() {
    let sequence = "GATCGATCCTAG";
    let flat = import_all(&container(1, 15, &base_packets(0x01, sequence)));
    let compressed = import_all(&container(1, 15, &base_packets_2bit(0x01, sequence)));
    assert_eq!(flat.records[0].sequence, sequence);
    assert_eq!(compressed.records[0].sequence, sequence);
}

fn base_packets_2bit(flags: u8, bases: &str) -> Vec<(u8, Vec<u8>)> {
    vec![
        (1, dna_2bit(flags, bases)),
        (
            6,
            notes_xml("<CustomMapLabel>synthetic plasmid</CustomMapLabel><UseCustomMapLabel>1</UseCustomMapLabel>")
                .into_bytes(),
        ),
    ]
}

#[test]
fn missing_name_is_a_required_field_error() {
    let bytes = container(1, 15, &[(0, dna(0x00, "ACGT"))]);
    let error = import_error(&bytes);
    assert_eq!(error.code(), "RECORD_REQUIRED_FIELD_MISSING");
    assert!(matches!(
        error,
        ImportError::MissingRequired {
            field: sequon_io::RequiredField::Name,
            ..
        }
    ));
}

#[test]
fn name_hint_fills_a_missing_name() {
    let bytes = container(1, 15, &[(0, dna(0x00, "ACGTACGT"))]);
    let mut options = default_options();
    options.name_hint = Some("caller hint".to_string());
    let outcome = sequon_io::import(&bytes, &options).expect("import must succeed");
    assert_eq!(outcome.records[0].name, "caller hint");
}

#[test]
fn name_precedence_prefers_accession_then_custom_label_then_comments_then_hint() {
    let notes = "<AccessionNumber>ACC.1</AccessionNumber><CustomMapLabel>LABEL</CustomMapLabel><UseCustomMapLabel>1</UseCustomMapLabel><Comments>COMMENT</Comments>";
    let bytes = container(
        1,
        15,
        &[
            (0, dna(0x00, "ACGTACGT")),
            (6, notes_xml(notes).into_bytes()),
        ],
    );
    assert_eq!(import_all(&bytes).records[0].name, "ACC.1");

    let notes = "<CustomMapLabel>LABEL</CustomMapLabel><UseCustomMapLabel>1</UseCustomMapLabel><Comments>COMMENT</Comments>";
    let bytes = container(
        1,
        15,
        &[
            (0, dna(0x00, "ACGTACGT")),
            (6, notes_xml(notes).into_bytes()),
        ],
    );
    assert_eq!(import_all(&bytes).records[0].name, "LABEL");

    let notes = "<CustomMapLabel>LABEL</CustomMapLabel><UseCustomMapLabel>0</UseCustomMapLabel><Comments>COMMENT</Comments>";
    let bytes = container(
        1,
        15,
        &[
            (0, dna(0x00, "ACGTACGT")),
            (6, notes_xml(notes).into_bytes()),
        ],
    );
    assert_eq!(import_all(&bytes).records[0].name, "COMMENT");
}

#[test]
fn features_map_ranges_strands_and_qualifiers() {
    let inner = r##"<Feature recentID="0" name="MCS" type="misc_feature"><Segment range="2-5" color="#99ccff"/><Q name="note"><V text="&lt;html&gt;&lt;body&gt;the &lt;i&gt;site&lt;/i&gt;&lt;/body&gt;&lt;/html&gt;"/></Q></Feature>
        <Feature recentID="1" name="T7" type="promoter" directionality="2"><Segment range="7-9"/><Segment range="11-12"/><Q name="transl_table"><V int="1"/></Q></Feature>"##;
    let bytes = container(
        1,
        15,
        &[
            (0, dna(0x00, "AACCGGTTAACCGG")),
            (10, features_xml(inner).into_bytes()),
            (
                6,
                notes_xml(
                    "<CustomMapLabel>x</CustomMapLabel><UseCustomMapLabel>1</UseCustomMapLabel>",
                )
                .into_bytes(),
            ),
        ],
    );
    let record = &import_all(&bytes).records[0];
    assert_eq!(record.features.len(), 2);
    assert_eq!(record.features[0].key, "misc_feature");
    assert_eq!(record.features[0].name.as_deref(), Some("MCS"));
    assert_eq!(record.features[0].location.strand, 1);
    assert_eq!(record.features[0].location.operator, "span");
    assert_eq!(record.features[0].location.segments[0].start, 1);
    assert_eq!(record.features[0].location.segments[0].end, 5);
    assert_eq!(
        record.features[0].qualifiers[0].value.as_deref(),
        Some("the site")
    );
    assert_eq!(record.features[1].location.operator, "join");
    assert_eq!(record.features[1].location.strand, -1);
    assert_eq!(record.features[1].location.segments[0].start, 6);
    assert_eq!(record.features[1].location.segments[1].start, 10);
    assert_eq!(record.features[1].qualifiers[0].value.as_deref(), Some("1"));

    let extras = record
        .extras
        .get("snapgene")
        .and_then(sequon_model::JsonValue::as_object)
        .expect("snapgene");
    let features = extras
        .get("features")
        .and_then(sequon_model::JsonValue::as_array)
        .expect("features");
    let first = features[0].as_object().expect("entry");
    let qualifiers = first
        .get("qualifiers")
        .and_then(sequon_model::JsonValue::as_array)
        .expect("raw qualifiers");
    assert_eq!(
        qualifiers[0]
            .as_object()
            .and_then(|object| object.get("raw"))
            .and_then(sequon_model::JsonValue::as_str),
        Some("<html><body>the <i>site</i></body></html>")
    );
}

#[test]
fn optional_metadata_blocks_are_preserved_with_info() {
    let restriction = vec![1, 0, 0, 0, 7, b'G', b'A', b'T', b'C', 0, 0];
    let settings = vec![0_u8; 24];
    let bytes = container(
        1,
        15,
        &[
            (0, dna(0x01, "ACGTACGT")),
            (3, restriction),
            (13, settings),
            (16, vec![1, 2, 3]),
            (17, b"<AlignableSequences/>".to_vec()),
            (42, vec![9, 9, 9]),
            (
                6,
                notes_xml(
                    "<CustomMapLabel>x</CustomMapLabel><UseCustomMapLabel>1</UseCustomMapLabel>",
                )
                .into_bytes(),
            ),
        ],
    );
    let outcome = import_all(&bytes);
    assert_eq!(outcome.report.preserved.len(), 5);
    let codes: Vec<&str> = outcome
        .report
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect();
    assert_eq!(
        codes,
        vec![
            "DNA_PACKET_IGNORED",
            "DNA_PACKET_IGNORED",
            "DNA_PACKET_IGNORED",
            "DNA_PACKET_IGNORED",
            "DNA_UNKNOWN_PACKET",
        ]
    );
    assert!(outcome.report.dropped.is_empty());
}

#[test]
fn preservation_disabled_fingerprints_with_not_requested() {
    let bytes = container(
        1,
        15,
        &[
            (0, dna(0x01, "ACGTACGT")),
            (3, vec![1, 0, 0, 0, 0]),
            (
                6,
                notes_xml(
                    "<CustomMapLabel>x</CustomMapLabel><UseCustomMapLabel>1</UseCustomMapLabel>",
                )
                .into_bytes(),
            ),
        ],
    );
    let mut options = default_options();
    options.preserve_unhandled_blocks = false;
    let outcome = sequon_io::import(&bytes, &options).expect("import must succeed");
    assert!(outcome.report.preserved.is_empty());
    assert_eq!(outcome.report.dropped.len(), 1);
    assert_eq!(outcome.report.dropped[0].reason, "not_requested");
    assert_eq!(outcome.report.dropped[0].what, "restriction_site_list");
    assert!(outcome.report.dropped[0].digest.is_some());
}

#[test]
fn history_plain_xml_and_lzma_paths_are_preserved() {
    let history = b"<History><Step/></History>";
    let bytes = container(
        1,
        14,
        &[
            (0, dna(0x01, "ACGTACGT")),
            (7, history.to_vec()),
            (
                6,
                notes_xml(
                    "<CustomMapLabel>x</CustomMapLabel><UseCustomMapLabel>1</UseCustomMapLabel>",
                )
                .into_bytes(),
            ),
        ],
    );
    let outcome = import_all(&bytes);
    assert!(outcome
        .report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "DNA_HISTORY_PRESERVED_RAW"));
    assert_eq!(outcome.report.preserved.len(), 1);
    let extras = outcome.records[0]
        .extras
        .get("snapgene")
        .and_then(sequon_model::JsonValue::as_object)
        .expect("snapgene");
    let history_metadata = extras
        .get("history")
        .and_then(sequon_model::JsonValue::as_object)
        .expect("history");
    assert_eq!(
        history_metadata
            .get("encoding")
            .and_then(sequon_model::JsonValue::as_str),
        Some("xml")
    );
    assert_eq!(
        history_metadata
            .get("decoded")
            .and_then(sequon_model::JsonValue::as_bool),
        Some(true)
    );
}

#[test]
fn history_modifier_type_29_uses_the_same_version_rule() {
    let bytes = container(
        1,
        14,
        &[
            (0, dna(0x01, "ACGTACGT")),
            (29, b"<History><Step/></History>".to_vec()),
            (
                6,
                notes_xml(
                    "<CustomMapLabel>x</CustomMapLabel><UseCustomMapLabel>1</UseCustomMapLabel>",
                )
                .into_bytes(),
            ),
        ],
    );
    let outcome = import_all(&bytes);
    assert!(outcome
        .report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "DNA_HISTORY_PRESERVED_RAW"));
    assert_eq!(outcome.report.preserved.len(), 1);
}

#[test]
fn history_malformed_is_a_warning_not_a_failure() {
    let bytes = container(
        1,
        14,
        &[
            (0, dna(0x01, "ACGTACGT")),
            (7, b"<History>".to_vec()),
            (
                6,
                notes_xml(
                    "<CustomMapLabel>x</CustomMapLabel><UseCustomMapLabel>1</UseCustomMapLabel>",
                )
                .into_bytes(),
            ),
        ],
    );
    let outcome = import_all(&bytes);
    assert!(outcome
        .report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "DNA_HISTORY_DECODE_FAILED"));
    assert_eq!(outcome.report.preserved.len(), 1);
}

#[test]
fn malformed_feature_metadata_is_skipped_and_reported() {
    let inner = r##"<Feature name="broken"><Segment range="1-4"/></Feature>"##;
    let bytes = container(
        1,
        15,
        &[
            (0, dna(0x00, "ACGTACGT")),
            (10, features_xml(inner).into_bytes()),
            (
                6,
                notes_xml(
                    "<CustomMapLabel>x</CustomMapLabel><UseCustomMapLabel>1</UseCustomMapLabel>",
                )
                .into_bytes(),
            ),
        ],
    );
    let outcome = import_all(&bytes);
    assert!(outcome.records[0].features.is_empty());
    assert_eq!(outcome.report.dropped.len(), 1);
    assert_eq!(outcome.report.dropped[0].reason, "malformed_metadata");
    assert!(outcome
        .report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "DNA_FEATURE_MALFORMED"));
}

#[test]
fn out_of_range_feature_is_skipped_with_a_warning() {
    let inner = r##"<Feature type="CDS"><Segment range="90-120"/></Feature>"##;
    let bytes = container(
        1,
        15,
        &[
            (0, dna(0x00, "ACGTACGT")),
            (10, features_xml(inner).into_bytes()),
            (
                6,
                notes_xml(
                    "<CustomMapLabel>x</CustomMapLabel><UseCustomMapLabel>1</UseCustomMapLabel>",
                )
                .into_bytes(),
            ),
        ],
    );
    let outcome = import_all(&bytes);
    assert!(outcome.records[0].features.is_empty());
    assert_eq!(outcome.report.dropped[0].reason, "coordinate_out_of_range");
    assert!(outcome
        .report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "DNA_FEATURE_RANGE_INVALID"));
}

#[test]
fn circular_cross_origin_feature_stays_unwrapped() {
    let inner = r##"<Feature type="CDS"><Segment range="7-10"/></Feature>"##;
    let bytes = container(
        1,
        15,
        &[
            (0, dna(0x01, "ACGTACGT")),
            (10, features_xml(inner).into_bytes()),
            (
                6,
                notes_xml(
                    "<CustomMapLabel>x</CustomMapLabel><UseCustomMapLabel>1</UseCustomMapLabel>",
                )
                .into_bytes(),
            ),
        ],
    );
    let outcome = import_all(&bytes);
    let location = &outcome.records[0].features[0].location;
    assert_eq!(location.operator, "span");
    assert_eq!(location.segments.len(), 1);
    assert_eq!(location.segments[0].start, 6);
    assert_eq!(location.segments[0].end, 10);
    assert!(location.is_wrapped(8));
    assert_eq!(
        location.resolved_segments(8),
        vec![
            sequon_model::Segment::new(6, 8),
            sequon_model::Segment::new(0, 2),
        ]
    );
}

#[test]
fn properties_topology_conflict_is_reported_and_flags_win() {
    let bytes = container(
        1,
        15,
        &[
            (0, dna(0x01, "ACGTACGT")),
            (8, properties_xml("<IsCircular>0</IsCircular>").into_bytes()),
            (
                6,
                notes_xml(
                    "<CustomMapLabel>x</CustomMapLabel><UseCustomMapLabel>1</UseCustomMapLabel>",
                )
                .into_bytes(),
            ),
        ],
    );
    let outcome = import_all(&bytes);
    assert_eq!(
        outcome.records[0].topology,
        sequon_model::Topology::Circular
    );
    assert!(outcome
        .report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "DNA_TOPOLOGY_CONFLICT"));
}

#[test]
fn empty_primers_are_kept_and_settings_are_preserved() {
    let bytes = container(
        1,
        15,
        &[
            (0, dna(0x01, "ACGTACGT")),
            (5, empty_primers_xml().into_bytes()),
            (
                6,
                notes_xml(
                    "<CustomMapLabel>x</CustomMapLabel><UseCustomMapLabel>1</UseCustomMapLabel>",
                )
                .into_bytes(),
            ),
        ],
    );
    let outcome = import_all(&bytes);
    assert!(outcome.records[0].features.is_empty());
    let extras = outcome.records[0]
        .extras
        .get("snapgene")
        .and_then(sequon_model::JsonValue::as_object)
        .expect("snapgene");
    assert!(extras
        .get("primers")
        .and_then(sequon_model::JsonValue::as_array)
        .is_some());
    assert_eq!(
        extras
            .get("primer_settings")
            .and_then(sequon_model::JsonValue::as_array)
            .map(Vec::len),
        Some(1)
    );
}

#[test]
fn hypothesis_primer_shape_becomes_a_primer_bind_feature() {
    let primers = r##"<Primers><Primer name="M13F" sequence="GTAAAACGACGGCCAGT" binding_site="1-17" directionality="1"/></Primers>"##;
    let bytes = container(
        1,
        15,
        &[
            (0, dna(0x01, "ACGTACGTACGTACGTACGT")),
            (5, primers.as_bytes().to_vec()),
            (
                6,
                notes_xml(
                    "<CustomMapLabel>x</CustomMapLabel><UseCustomMapLabel>1</UseCustomMapLabel>",
                )
                .into_bytes(),
            ),
        ],
    );
    let outcome = import_all(&bytes);
    let feature = &outcome.records[0].features[0];
    assert_eq!(feature.key, "primer_bind");
    assert_eq!(feature.name.as_deref(), Some("M13F"));
    assert_eq!(feature.location.strand, 1);
    assert_eq!(feature.location.segments[0].start, 0);
    assert_eq!(feature.location.segments[0].end, 17);
}

#[test]
fn deep_xml_is_rejected_with_the_depth_code() {
    let mut inner = String::new();
    for _ in 0..80 {
        inner.push_str("<a>");
    }
    for _ in 0..80 {
        inner.push_str("</a>");
    }
    let features = format!(
        "<Features><Feature type=\"CDS\"><Segment range=\"1-2\"/>{inner}</Feature></Features>"
    );
    let bytes = container(
        1,
        15,
        &[
            (0, dna(0x00, "ACGTACGT")),
            (10, features.into_bytes()),
            (
                6,
                notes_xml(
                    "<CustomMapLabel>x</CustomMapLabel><UseCustomMapLabel>1</UseCustomMapLabel>",
                )
                .into_bytes(),
            ),
        ],
    );
    assert_eq!(import_error(&bytes).code(), "DNA_XML_DEPTH_EXCEEDED");
}

#[test]
fn trailing_non_zero_bytes_are_a_warning() {
    let mut bytes = container(
        1,
        15,
        &[
            (0, dna(0x00, "ACGTACGT")),
            (
                6,
                notes_xml(
                    "<CustomMapLabel>x</CustomMapLabel><UseCustomMapLabel>1</UseCustomMapLabel>",
                )
                .into_bytes(),
            ),
        ],
    );
    bytes.extend_from_slice(&[1, 2]);
    let outcome = import_all(&bytes);
    assert!(outcome
        .report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "DNA_TRAILING_BYTES"));
}

#[test]
fn reports_use_absolute_snapgene_positions() {
    let inner = r##"<Feature name="broken"><Segment range="1-4"/></Feature>"##;
    let bytes = container(
        1,
        15,
        &[
            (0, dna(0x00, "ACGTACGT")),
            (10, features_xml(inner).into_bytes()),
            (
                6,
                notes_xml(
                    "<CustomMapLabel>x</CustomMapLabel><UseCustomMapLabel>1</UseCustomMapLabel>",
                )
                .into_bytes(),
            ),
        ],
    );
    let outcome = import_all(&bytes);
    match outcome.report.diagnostics[0].position.as_ref() {
        Some(SourcePosition::Snapgene {
            offset,
            packet_type,
            packet_index,
        }) => {
            assert_eq!(*packet_type, Some(10));
            assert_eq!(*packet_index, Some(2));
            assert_eq!(*offset, 19 + 5 + 9);
        }
        other => panic!("unexpected position: {other:?}"),
    }
}

#[test]
fn import_is_deterministic() {
    let bytes = container(
        1,
        15,
        &[
            (0, dna(0x1f, "ACGTACGTACGT")),
            (
                10,
                features_xml(r##"<Feature type="CDS"><Segment range="2-4"/></Feature>"##)
                    .into_bytes(),
            ),
            (
                6,
                notes_xml(
                    "<CustomMapLabel>x</CustomMapLabel><UseCustomMapLabel>1</UseCustomMapLabel>",
                )
                .into_bytes(),
            ),
        ],
    );
    let first = import_all(&bytes);
    let second = import_all(&bytes);
    let first_json = sequon_io::json::to_canonical_json_bytes(&first.records[0]).expect("json");
    let second_json = sequon_io::json::to_canonical_json_bytes(&second.records[0]).expect("json");
    assert_eq!(first_json, second_json);
    let first_report =
        sequon_io::json::to_canonical_report_json_bytes(&first.report).expect("json");
    let second_report =
        sequon_io::json::to_canonical_report_json_bytes(&second.report).expect("json");
    assert_eq!(first_report, second_report);
}

#[test]
fn packet_limit_is_enforced() {
    let mut packets = vec![(0, dna(0x00, "ACGTACGT"))];
    for _ in 0..8 {
        packets.push((42, vec![0]));
    }
    let bytes = container(1, 15, &packets);
    let mut options = default_options();
    options.limits.max_snapgene_packets = 4;
    let error = sequon_io::import(&bytes, &options).expect_err("limit");
    assert_eq!(error.code(), "LIMIT_EXCEEDED");
}

#[test]
fn preserved_byte_budget_fingerprints_the_overflow() {
    let bytes = container(
        1,
        15,
        &[
            (0, dna(0x01, "ACGTACGT")),
            (3, vec![1, 0, 0, 0, 0, b'A', b'C', b'G', b'T']),
            (
                6,
                notes_xml(
                    "<CustomMapLabel>x</CustomMapLabel><UseCustomMapLabel>1</UseCustomMapLabel>",
                )
                .into_bytes(),
            ),
        ],
    );
    let mut options = default_options();
    options.limits.max_preserved_block_bytes = 4;
    let outcome = sequon_io::import(&bytes, &options).expect("import must succeed");
    assert!(outcome.report.preserved.is_empty());
    assert_eq!(outcome.report.dropped.len(), 1);
    assert_eq!(outcome.report.dropped[0].reason, "size_limit");
}

#[test]
fn malformed_required_xml_is_a_typed_error() {
    let bytes = container(
        1,
        15,
        &[
            (0, dna(0x00, "ACGTACGT")),
            (10, b"<Features><Feature type=\"CDS\">".to_vec()),
            (
                6,
                notes_xml(
                    "<CustomMapLabel>x</CustomMapLabel><UseCustomMapLabel>1</UseCustomMapLabel>",
                )
                .into_bytes(),
            ),
        ],
    );
    assert_eq!(import_error(&bytes).code(), "DNA_XML_MALFORMED");
}

#[test]
fn history_lzma_path_is_selected_by_export_version() {
    let history_xml = b"<History><Step/></History>";
    let compressed = lzma_alone(history_xml);
    let bytes = container(
        1,
        17,
        &[
            (0, dna(0x01, "ACGTACGT")),
            (7, compressed),
            (
                6,
                notes_xml(
                    "<CustomMapLabel>x</CustomMapLabel><UseCustomMapLabel>1</UseCustomMapLabel>",
                )
                .into_bytes(),
            ),
        ],
    );
    let outcome = import_all(&bytes);
    assert!(outcome
        .report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "DNA_HISTORY_PRESERVED_RAW"));
    let extras = outcome.records[0]
        .extras
        .get("snapgene")
        .and_then(sequon_model::JsonValue::as_object)
        .expect("snapgene");
    let metadata = extras
        .get("history")
        .and_then(sequon_model::JsonValue::as_object)
        .expect("history");
    assert_eq!(
        metadata
            .get("encoding")
            .and_then(sequon_model::JsonValue::as_str),
        Some("lzma")
    );
    assert_eq!(
        metadata
            .get("decoded")
            .and_then(sequon_model::JsonValue::as_bool),
        Some(true)
    );
}

fn lzma_alone(data: &[u8]) -> Vec<u8> {
    let options = lzma_rs::compress::Options {
        unpacked_size: lzma_rs::compress::UnpackedSize::WriteToHeader(Some(data.len() as u64)),
    };
    let mut output = Vec::new();
    lzma_rs::lzma_compress_with_options(&mut &data[..], &mut output, &options)
        .expect("in-memory compression");
    output
}

#[test]
fn xml_node_cap_is_enforced_with_a_snapgene_position() {
    let notes =
        notes_xml("<CustomMapLabel>x</CustomMapLabel><UseCustomMapLabel>1</UseCustomMapLabel>");
    let bytes = container(
        1,
        15,
        &[(0, dna(0x00, "ACGTACGT")), (6, notes.into_bytes())],
    );
    let mut options = default_options();
    options.limits = sequon_io::Limits {
        max_xml_nodes: 2,
        ..sequon_io::Limits::default()
    };
    let error = sequon_io::import(&bytes, &options).expect_err("node limit");
    assert_eq!(error.code(), "LIMIT_EXCEEDED");
    assert!(matches!(
        error.position(),
        Some(SourcePosition::Snapgene { .. })
    ));
}

#[test]
fn xml_byte_budget_spans_all_xml_blocks() {
    let notes =
        notes_xml("<CustomMapLabel>x</CustomMapLabel><UseCustomMapLabel>1</UseCustomMapLabel>");
    let properties = properties_xml("<IsCircular>1</IsCircular>");
    let notes_len = notes.len() as u64;
    let properties_len = properties.len() as u64;
    let bytes = container(
        1,
        15,
        &[
            (0, dna(0x00, "ACGTACGT")),
            (6, notes.into_bytes()),
            (8, properties.into_bytes()),
        ],
    );
    let mut options = default_options();
    options.limits = sequon_io::Limits {
        max_xml_bytes: notes_len + properties_len - 1,
        ..sequon_io::Limits::default()
    };
    let error = sequon_io::import(&bytes, &options).expect_err("byte limit");
    assert_eq!(error.code(), "LIMIT_EXCEEDED");
    match error.position() {
        Some(SourcePosition::Snapgene {
            offset,
            packet_type,
            ..
        }) => {
            let properties_offset = 19 + 14 + 5 + notes_len;
            assert_eq!(*packet_type, Some(8));
            assert_eq!(*offset, properties_offset);
        }
        other => panic!("unexpected position: {other:?}"),
    }
    let mut options = default_options();
    options.limits = sequon_io::Limits {
        max_xml_bytes: notes_len + properties_len,
        ..sequon_io::Limits::default()
    };
    sequon_io::import(&bytes, &options).expect("exact budget must pass");
}

#[test]
fn history_plain_xml_counts_against_the_xml_byte_budget() {
    let history = b"<History><Step/></History>";
    let bytes = container(
        1,
        14,
        &[
            (0, dna(0x01, "ACGTACGT")),
            (7, history.to_vec()),
            (
                6,
                notes_xml("<CustomMapLabel>x</CustomMapLabel>").into_bytes(),
            ),
        ],
    );
    let mut options = default_options();
    options.limits = sequon_io::Limits {
        max_xml_bytes: history.len() as u64 - 1,
        ..sequon_io::Limits::default()
    };
    let error = sequon_io::import(&bytes, &options).expect_err("history byte limit");
    assert_eq!(error.code(), "LIMIT_EXCEEDED");
    assert!(matches!(
        error.position(),
        Some(SourcePosition::Snapgene {
            packet_type: Some(7),
            ..
        })
    ));
}

#[test]
fn history_lzma_decoded_bytes_count_against_the_xml_byte_budget() {
    let history = b"<History><Step/></History>";
    let compressed = lzma_alone(history);
    let bytes = container(
        1,
        17,
        &[
            (0, dna(0x01, "ACGTACGT")),
            (7, compressed),
            (
                6,
                notes_xml("<CustomMapLabel>x</CustomMapLabel>").into_bytes(),
            ),
        ],
    );
    let mut options = default_options();
    options.limits = sequon_io::Limits {
        max_xml_bytes: history.len() as u64 - 1,
        ..sequon_io::Limits::default()
    };
    let error = sequon_io::import(&bytes, &options).expect_err("decoded byte limit");
    assert_eq!(error.code(), "LIMIT_EXCEEDED");
}
