#![cfg(feature = "genbank")]

//! Cross-cutting GenBank reader tests: policies, limits, diagnostics, encoding,
//! skipped features, determinism and the diagnostic severity contract.

use std::fs;
use std::path::PathBuf;

use sequon_io::api::{ImportOptions, MultiplePolicy};
use sequon_io::import_genbank;
use sequon_io::json::{to_canonical_json_bytes, to_canonical_report_json_bytes};
use sequon_io::report::{Severity, SourcePosition};
use sequon_io::ImportError;
use sequon_model::{ProducerKind, SourceFormat};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata")
}

fn fixture(name: &str) -> Vec<u8> {
    fs::read(root().join("genbank").join(format!("{name}.gb"))).expect("fixture file")
}

fn locus(name: &str, length: usize) -> String {
    format!("LOCUS       {name:<16} {length:>11} bp    DNA       linear SYN 01-JAN-2000")
}

fn origin(sequence: &str) -> String {
    let mut lines = vec!["ORIGIN".to_string()];
    for (index, chunk) in sequence.as_bytes().chunks(60).enumerate() {
        let text = std::str::from_utf8(chunk).expect("ASCII sequence");
        let groups: Vec<&str> = text
            .as_bytes()
            .chunks(10)
            .map(|group| std::str::from_utf8(group).expect("ASCII group"))
            .collect();
        lines.push(format!("{:>9} {}", index * 60 + 1, groups.join(" ")));
    }
    lines.join("\n")
}

fn feature(key: &str, location: &str, qualifiers: &[&str]) -> String {
    let mut line = format!("     {key:<15} {location}");
    for qualifier in qualifiers {
        line.push_str(&format!("\n                     {qualifier}"));
    }
    line
}

fn record(name: &str, sequence: &str, features: &[String], header: &[(&str, &str)]) -> Vec<u8> {
    let mut text = locus(name, sequence.len());
    for (key, value) in header {
        text.push('\n');
        text.push_str(&format!("{key:<12}{value}"));
    }
    text.push_str("\nFEATURES             Location/Qualifiers");
    for feature in features {
        text.push('\n');
        text.push_str(feature);
    }
    text.push('\n');
    text.push_str(&origin(sequence));
    text.push_str("\n//\n");
    text.into_bytes()
}

fn simple_record() -> Vec<u8> {
    record(
        "SIMPLE",
        "ATGTTCCATCCGAGGAACCAAGTACAGAGGCTTAATCAAAACGCTGGGGACAGCAAAGTGATTTCATGTCACCGACAAATTAGAACTCCGAGGATCAAAAGTGATTTCATGAACATTTAA",
        &[feature("CDS", "1..120", &["/gene=\"simple\""])],
        &[("DEFINITION", "simple synthetic record.")],
    )
}

fn codes(outcome: &sequon_io::api::ImportOutcome) -> Vec<&str> {
    outcome
        .report
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect()
}

fn severity_of(code: &str) -> Severity {
    match code {
        "GB_EMPTY_INPUT"
        | "GB_CONTENT_BEFORE_LOCUS"
        | "GB_MISSING_LOCUS"
        | "GB_MALFORMED_LOCUS"
        | "GB_RECORD_UNTERMINATED"
        | "GB_MISSING_ORIGIN"
        | "GB_SEQUENCE_EMPTY"
        | "GB_ORIGIN_INVALID_CHAR"
        | "GB_RNA_BASE_IN_DNA"
        | "GB_UNSUPPORTED_MOLECULE"
        | "GB_TOPOLOGY_MISSING"
        | "GB_LOCATION_MALFORMED"
        | "GB_MULTIPLE_RECORDS_NOT_ALLOWED"
        | "GB_ENCODING_INVALID"
        | "GB_FEATURES_MALFORMED"
        | "RECORD_REQUIRED_FIELD_MISSING"
        | "RECORD_FIELD_INVALID"
        | "LIMIT_EXCEEDED" => Severity::Error,
        "GB_LOCUS_LENGTH_MISMATCH"
        | "GB_LOCATION_UNSUPPORTED"
        | "GB_COORDINATE_OUT_OF_RANGE"
        | "MOLECULE_TYPE_UNKNOWN" => Severity::Warning,
        _ => Severity::Info,
    }
}

#[test]
fn report_envelope_matches_the_contract() {
    let bytes = fixture("001-minimal-cds");
    let outcome = import_genbank(&bytes, &ImportOptions::default()).expect("001 imports");
    let report = &outcome.report;
    assert_eq!(report.schema, "sequon/import-report/1");
    assert_eq!(report.source_format, SourceFormat::Genbank);
    assert_eq!(report.source_bytes, bytes.len() as u64);
    assert_eq!(
        report.source_digest.as_str(),
        "sha256:091e042ff261d6d279ee3511be52f6dfe5c4ad05ec430bfbfe400d0522d1f13c"
    );
    assert_eq!(report.parser.name, "sequon-io");
    assert_eq!(report.parser.version, "0.1.0");
    assert_eq!(report.parser.kind, ProducerKind::Parser);
    assert_eq!(report.codes_version, "2026-10-21/1");
    assert_eq!(report.export_version, None);
    assert_eq!(report.records.len(), 1);
    assert_eq!(report.records[0].index, 0);
    assert_eq!(report.records[0].name, "SYN001");
    assert_eq!(report.records[0].length, 60);
    assert_eq!(
        report.records[0].sequence_digest,
        outcome.records[0].digests.sequence
    );
    assert_eq!(
        report.records[0].canonical_digest,
        outcome.records[0].digests.canonical
    );
    assert!(report.diagnostics.is_empty());
    assert!(report.dropped.is_empty());
    assert!(report.preserved.is_empty());
}

#[test]
fn multi_record_policies() {
    let bytes = fixture("080-multi-record");
    let all = import_genbank(&bytes, &ImportOptions::default()).expect("All");
    assert_eq!(all.records.len(), 2);

    let first = ImportOptions {
        multiple: MultiplePolicy::First,
        ..ImportOptions::default()
    };
    let outcome = import_genbank(&bytes, &first).expect("First");
    assert_eq!(outcome.records.len(), 1);
    assert_eq!(outcome.records[0].name, "SYN080A");

    let reject = ImportOptions {
        multiple: MultiplePolicy::Reject,
        ..ImportOptions::default()
    };
    let error = import_genbank(&bytes, &reject).expect_err("Reject");
    assert_eq!(error.code(), "GB_MULTIPLE_RECORDS_NOT_ALLOWED");
    assert_eq!(
        error.position(),
        Some(&SourcePosition::Genbank {
            line: 9,
            column: None
        })
    );
}

#[test]
fn a_later_record_failure_is_an_error_diagnostic_under_all_and_first() {
    let good = simple_record();
    let bad = record(
        "BAD",
        "ATGTTCCATCCGAGGAACCAAGTACAGAGGCTTAATCAAAACGCTGGGGACAGCAAAGTGATTTCATGTCACCGACAAATTAGAACTCCGAGGATCAAAAGTGATTTCATGAACATTTAA",
        &[feature("CDS", "1..(120", &[])],
        &[],
    );
    let mut bytes = good;
    bytes.extend_from_slice(&bad);
    let outcome = import_genbank(&bytes, &ImportOptions::default()).expect("All with a failure");
    assert_eq!(outcome.records.len(), 1);
    let diagnostic = outcome
        .report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "GB_LOCATION_MALFORMED")
        .expect("error diagnostic");
    assert_eq!(diagnostic.severity, Severity::Error);
    assert_eq!(diagnostic.record_index, Some(1));

    let first = ImportOptions {
        multiple: MultiplePolicy::First,
        ..ImportOptions::default()
    };
    let outcome = import_genbank(&bytes, &first).expect("First with a later failure");
    assert_eq!(outcome.records.len(), 1);
    assert_eq!(outcome.report.records.len(), 1);
    let diagnostic = outcome
        .report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "GB_LOCATION_MALFORMED")
        .expect("error diagnostic");
    assert_eq!(diagnostic.record_index, Some(1));
    assert!(outcome
        .report
        .records
        .iter()
        .all(|summary| summary.index == 0));
}

#[test]
fn a_failed_first_record_under_first_returns_err() {
    let bytes = record(
        "BAD",
        "ATGTTCCATCCGAGGAACCAAGTACAGAGGCTTAATCAAAACGCTGGGGACAGCAAAGTGATTTCATGTCACCGACAAATTAGAACTCCGAGGATCAAAAGTGATTTCATGAACATTTAA",
        &[feature("CDS", "1..(120", &[])],
        &[],
    );
    let options = ImportOptions {
        multiple: MultiplePolicy::First,
        ..ImportOptions::default()
    };
    let error = import_genbank(&bytes, &options).expect_err("First must fail when record 0 fails");
    assert_eq!(error.code(), "GB_LOCATION_MALFORMED");
}

#[test]
fn determinism_of_records_and_report() {
    for name in [
        "001-minimal-cds",
        "040-near-origin",
        "060-duplicate-qualifiers",
        "080-multi-record",
        "150-large-1mb",
    ] {
        let bytes = fixture(name);
        let first = import_genbank(&bytes, &ImportOptions::default()).expect("first");
        let second = import_genbank(&bytes, &ImportOptions::default()).expect("second");
        assert_eq!(first.records, second.records, "{name} records");
        assert_eq!(first.report, second.report, "{name} report");
        for record in &first.records {
            assert_eq!(
                to_canonical_json_bytes(record).expect("record json"),
                to_canonical_json_bytes(
                    &second.records[first
                        .records
                        .iter()
                        .position(|candidate| candidate == record)
                        .expect("record present")]
                )
                .expect("record json")
            );
        }
        assert_eq!(
            to_canonical_report_json_bytes(&first.report).expect("report json"),
            to_canonical_report_json_bytes(&second.report).expect("report json")
        );
    }
}

#[test]
fn limits_are_typed_errors_with_the_limit_name() {
    assert_limit("max_input_bytes", fixture("001-minimal-cds"), |options| {
        options.limits.max_input_bytes = 10;
    });
    assert_limit("max_sequence_len", fixture("001-minimal-cds"), |options| {
        options.limits.max_sequence_len = 30;
    });
    assert_limit("max_features", fixture("001-minimal-cds"), |options| {
        options.limits.max_features = 1;
    });
    assert_limit("max_header_bytes", fixture("001-minimal-cds"), |options| {
        options.limits.max_header_bytes = 10;
    });
    assert_limit(
        "max_qualifier_value_bytes",
        fixture("001-minimal-cds"),
        |options| options.limits.max_qualifier_value_bytes = 5,
    );
    assert_limit(
        "max_segments_per_feature",
        fixture("030-order"),
        |options| options.limits.max_segments_per_feature = 2,
    );
    assert_limit("max_records", fixture("080-multi-record"), |options| {
        options.limits.max_records = 1;
    });
    assert_limit(
        "max_sequence_len",
        simple_record_with_location("1..2000"),
        |options| options.limits.max_sequence_len = 1000,
    );
    let deep = simple_record_with_location("join(join(join(1..2,3..4),5..6),7..8)");
    assert_limit("max_location_depth", deep, |options| {
        options.limits.max_location_depth = 1;
    });
}

fn assert_limit(
    expected: &'static str,
    bytes: Vec<u8>,
    configure: impl FnOnce(&mut ImportOptions),
) {
    let mut options = ImportOptions::default();
    configure(&mut options);
    match import_genbank(&bytes, &options) {
        Err(ImportError::LimitExceeded { limit, .. }) => assert_eq!(limit, expected),
        other => panic!("expected limit {expected}, got {other:?}"),
    }
}

#[test]
fn encoding_invalid_is_a_typed_error_with_a_line() {
    let mut bytes = locus("ENC", 60).into_bytes();
    bytes.extend_from_slice(b"\n\xff\n");
    let error = import_genbank(&bytes, &ImportOptions::default()).expect_err("invalid UTF-8");
    assert_eq!(error.code(), "GB_ENCODING_INVALID");
    match error.position() {
        Some(SourcePosition::Genbank { line, .. }) => assert_eq!(*line, 2),
        other => panic!("expected a GenBank line position, got {other:?}"),
    }
}

#[test]
fn empty_and_non_genbank_inputs_are_typed_errors() {
    assert_eq!(
        import_genbank(b"", &ImportOptions::default())
            .expect_err("empty")
            .code(),
        "GB_EMPTY_INPUT"
    );
    assert_eq!(
        import_genbank(b"   \n\n", &ImportOptions::default())
            .expect_err("whitespace")
            .code(),
        "GB_EMPTY_INPUT"
    );
    assert_eq!(
        import_genbank(b"plain text\n", &ImportOptions::default())
            .expect_err("no locus")
            .code(),
        "GB_MISSING_LOCUS"
    );
    assert_eq!(
        import_genbank(b"junk\nLOCUS       X\n", &ImportOptions::default())
            .expect_err("content before LOCUS")
            .code(),
        "GB_CONTENT_BEFORE_LOCUS"
    );
}

#[test]
fn malformed_locus_missing_origin_empty_sequence_and_bad_chars() {
    assert_eq!(
        import_genbank(b"LOCUS\n//\n", &ImportOptions::default())
            .expect_err("malformed LOCUS")
            .code(),
        "GB_MALFORMED_LOCUS"
    );
    let missing_origin = format!("{}\nDEFINITION  x.\n//\n", locus("X", 60)).into_bytes();
    assert_eq!(
        import_genbank(&missing_origin, &ImportOptions::default())
            .expect_err("missing ORIGIN")
            .code(),
        "GB_MISSING_ORIGIN"
    );
    let empty_sequence = record("EMPTY", "", &[], &[]);
    assert_eq!(
        import_genbank(&empty_sequence, &ImportOptions::default())
            .expect_err("empty sequence")
            .code(),
        "GB_SEQUENCE_EMPTY"
    );
    let invalid = record("INVALID", "ATG-X", &[], &[]);
    assert_eq!(
        import_genbank(&invalid, &ImportOptions::default())
            .expect_err("invalid base")
            .code(),
        "GB_ORIGIN_INVALID_CHAR"
    );
    let rna_base = record("RNABASE", "ATGU", &[], &[]);
    assert_eq!(
        import_genbank(&rna_base, &ImportOptions::default())
            .expect_err("U in DNA")
            .code(),
        "GB_RNA_BASE_IN_DNA"
    );
}

#[test]
fn structurally_broken_features_block_is_typed() {
    let mut text = String::from_utf8(record("BROKEN", "ATG", &[], &[])).expect("utf8");
    text = text.replace(
        "FEATURES             Location/Qualifiers",
        "FEATURES             Location/Qualifiers\n                     /note=\"stray\"",
    );
    let error =
        import_genbank(text.as_bytes(), &ImportOptions::default()).expect_err("stray qualifier");
    assert_eq!(error.code(), "GB_FEATURES_MALFORMED");
}

#[test]
fn unsupported_and_out_of_range_features_are_skipped_and_dropped() {
    let unsupported = record(
        "UNSUPPORTED",
        "ATGTTCCATCCGAGGAACCAAGTACAGAGGCTTAATCAAAACGCTGGGGACAGCAAAGTGATTTCATGTCACCGACAAATTAGAACTCCGAGGATCAAAAGTGATTTCATGAACATTTAA",
        &[feature("CDS", "one-of(1..2,3..4)", &["/gene=\"x\""]), feature("misc_feature", "1..10", &[])],
        &[],
    );
    let outcome = import_genbank(&unsupported, &ImportOptions::default()).expect("skipped feature");
    assert_eq!(outcome.records[0].features.len(), 1);
    assert_eq!(codes(&outcome), vec!["GB_LOCATION_UNSUPPORTED"]);
    assert_eq!(outcome.report.dropped.len(), 1);
    assert_eq!(outcome.report.dropped[0].what, "feature");
    assert_eq!(outcome.report.dropped[0].reason, "unsupported_location");
    assert_eq!(outcome.report.dropped[0].record_index, Some(0));
    assert_eq!(
        outcome.report.dropped[0].feature_key.as_deref(),
        Some("CDS")
    );
    assert_eq!(
        outcome.report.dropped[0].location_text.as_deref(),
        Some("one-of(1..2,3..4)")
    );

    let out_of_range = record(
        "RANGE",
        "ATG",
        &[feature("CDS", "1..200", &["/gene=\"x\""])],
        &[],
    );
    let outcome = import_genbank(&out_of_range, &ImportOptions::default()).expect("skipped range");
    assert!(outcome.records[0].features.is_empty());
    assert!(codes(&outcome).contains(&"GB_COORDINATE_OUT_OF_RANGE"));
    assert_eq!(outcome.report.dropped[0].reason, "coordinate_out_of_range");
}

#[test]
fn info_events_cover_qualifiers_between_positions_drift_normalization_and_headers() {
    let unquoted = record(
        "UNQUOTED",
        "ATG",
        &[feature("misc_feature", "1..3", &["/note=bare value"])],
        &[],
    );
    let outcome = import_genbank(&unquoted, &ImportOptions::default()).expect("unquoted");
    assert!(codes(&outcome).contains(&"GB_QUALIFIER_UNQUOTED"));

    let between = record(
        "BETWEEN",
        "ATGTTCCATCCGAGGAACCAAGTACAGAGGCTTAATCAAAACGCTGGGGACAGCAAAGTGATTTCATGTCACCGACAAATTAGAACTCCGAGGATCAAAAGTGATTTCATGAACATTTAA",
        &[feature("misc_feature", "100^101", &[])],
        &[],
    );
    let outcome = import_genbank(&between, &ImportOptions::default()).expect("between");
    assert!(codes(&outcome).contains(&"GB_LOCATION_BETWEEN_POSITIONS"));
    let segment = &outcome.records[0].features[0].location.segments[0];
    assert_eq!((segment.start, segment.end), (100, 100));

    let drifted = format!(
        "{}\nFEATURES             Location/Qualifiers\n  CDS{:16}1..3\nORIGIN\n        1 ATG\n//\n",
        locus("DRIFT", 3),
        ""
    );
    let outcome = import_genbank(drifted.as_bytes(), &ImportOptions::default()).expect("drift");
    assert!(codes(&outcome).contains(&"GB_FEATURE_COLUMN_MISALIGNED"));
    assert_eq!(outcome.records[0].features[0].key, "CDS");

    let normalized = record("NORM", "atg", &[], &[]);
    let outcome = import_genbank(&normalized, &ImportOptions::default()).expect("normalized");
    assert!(codes(&outcome).contains(&"GB_SEQUENCE_NORMALIZED"));
    assert_eq!(outcome.records[0].sequence, "ATG");

    let unknown_header = record(
        "HEADER",
        "ATG",
        &[],
        &[("SEGMENT", "1 of 2"), ("DEFINITION", "header test.")],
    );
    let outcome = import_genbank(&unknown_header, &ImportOptions::default()).expect("header");
    assert!(codes(&outcome).contains(&"GB_HEADER_FIELD_UNKNOWN"));
    let header = outcome.records[0]
        .extras
        .get("genbank")
        .expect("genbank extras")
        .as_object()
        .expect("object")
        .get("header")
        .expect("header")
        .as_array()
        .expect("array");
    assert_eq!(header.len(), 2);
}

#[test]
fn header_block_preserves_order_and_indented_organism() {
    let text = format!(
        "{}\nDEFINITION  header block.\nSOURCE      synthetic construct\n  ORGANISM  synthetic construct\n            synthetic; construct.\nREFERENCE   1  (bases 1 to 3)\n  AUTHORS   Doe,J.\n  TITLE     A title\n  JOURNAL   Unpublished\nCOMMENT     first comment line\n            second comment line\nFEATURES             Location/Qualifiers\n     CDS             1..3\nORIGIN\n        1 ATG\n//\n",
        locus("HDR", 3)
    );
    let outcome =
        import_genbank(text.as_bytes(), &ImportOptions::default()).expect("header record");
    assert_eq!(
        outcome.records[0].description.as_deref(),
        Some("header block.")
    );
    let genbank = outcome.records[0]
        .extras
        .get("genbank")
        .expect("genbank extras")
        .as_object()
        .expect("genbank object");
    let header = genbank
        .get("header")
        .expect("header")
        .as_array()
        .expect("header array");
    let entries: Vec<(String, String)> = header
        .iter()
        .map(|entry| {
            let object = entry.as_object().expect("header entry object");
            (
                object["key"].as_str().expect("key").to_string(),
                object["value"].as_str().expect("value").to_string(),
            )
        })
        .collect();
    let keys: Vec<&str> = entries.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(
        keys,
        vec!["DEFINITION", "SOURCE", "ORGANISM", "REFERENCE", "COMMENT"]
    );
    assert_eq!(
        entries[2].1,
        "synthetic construct\n            synthetic; construct."
    );
    assert_eq!(
        entries[3].1,
        "1  (bases 1 to 3)\n  AUTHORS   Doe,J.\n  TITLE     A title\n  JOURNAL   Unpublished"
    );
    assert_eq!(
        entries[4].1,
        "first comment line\n            second comment line"
    );
}

#[test]
fn record_field_invalid_for_unknown_topology() {
    let line = locus("TOPO", 3).replace("linear", "triangle");
    let text = format!("{line}\nORIGIN\n        1 ATG\n//\n");
    let error = import_genbank(text.as_bytes(), &ImportOptions::default()).expect_err("topology");
    assert_eq!(error.code(), "RECORD_FIELD_INVALID");
}

fn simple_record_with_location(location: &str) -> Vec<u8> {
    record(
        "LOCATION",
        "ATGTTCCATCCGAGGAACCAAGTACAGAGGCTTAATCAAAACGCTGGGGACAGCAAAGTGATTTCATGTCACCGACAAATTAGAACTCCGAGGATCAAAAGTGATTTCATGAACATTTAA",
        &[feature("misc_feature", location, &[])],
        &[],
    )
}

#[test]
fn every_exercised_gb_code_matches_the_documented_severity() {
    // Each case names an input that must produce the code. Cases are checked
    // against the severity fixed in docs/design/diagnostics.md section 9.1.
    let mut cases: Vec<(&str, Vec<u8>, ImportOptions)> = Vec::new();
    cases.push(("GB_EMPTY_INPUT", Vec::new(), ImportOptions::default()));
    cases.push((
        "GB_CONTENT_BEFORE_LOCUS",
        b"junk\nLOCUS       X\n".to_vec(),
        ImportOptions::default(),
    ));
    cases.push((
        "GB_MISSING_LOCUS",
        b"plain\n".to_vec(),
        ImportOptions::default(),
    ));
    cases.push((
        "GB_MALFORMED_LOCUS",
        b"LOCUS\n//\n".to_vec(),
        ImportOptions::default(),
    ));
    cases.push((
        "GB_RECORD_UNTERMINATED",
        fixture("120-unterminated"),
        ImportOptions::default(),
    ));
    cases.push((
        "GB_MISSING_ORIGIN",
        format!("{}\nDEFINITION  x.\n//\n", locus("X", 60)).into_bytes(),
        ImportOptions::default(),
    ));
    cases.push((
        "GB_SEQUENCE_EMPTY",
        record("EMPTY", "", &[], &[]),
        ImportOptions::default(),
    ));
    cases.push((
        "GB_ORIGIN_INVALID_CHAR",
        record("BAD", "AT-", &[], &[]),
        ImportOptions::default(),
    ));
    cases.push((
        "GB_RNA_BASE_IN_DNA",
        record("RNA", "ATU", &[], &[]),
        ImportOptions::default(),
    ));
    cases.push((
        "GB_UNSUPPORTED_MOLECULE",
        fixture("130-rna"),
        ImportOptions::default(),
    ));
    cases.push((
        "GB_TOPOLOGY_MISSING",
        fixture("161-missing-topology"),
        ImportOptions::default(),
    ));
    cases.push((
        "GB_LOCATION_MALFORMED",
        fixture("140-bad-location"),
        ImportOptions::default(),
    ));
    let reject = ImportOptions {
        multiple: MultiplePolicy::Reject,
        ..ImportOptions::default()
    };
    cases.push((
        "GB_MULTIPLE_RECORDS_NOT_ALLOWED",
        fixture("080-multi-record"),
        reject,
    ));
    cases.push((
        "GB_ENCODING_INVALID",
        b"LOCUS       X\n\xff\n".to_vec(),
        ImportOptions::default(),
    ));
    let mut broken = String::from_utf8(record("BROKEN", "ATG", &[], &[])).expect("utf8");
    broken = broken.replace(
        "FEATURES             Location/Qualifiers",
        "FEATURES             Location/Qualifiers\n                     /note=\"stray\"",
    );
    cases.push((
        "GB_FEATURES_MALFORMED",
        broken.into_bytes(),
        ImportOptions::default(),
    ));
    cases.push((
        "GB_LOCUS_LENGTH_MISMATCH",
        fixture("090-locus-length-mismatch"),
        ImportOptions::default(),
    ));
    cases.push((
        "GB_LOCATION_UNSUPPORTED",
        simple_record_with_location("one-of(1..2,3..4)"),
        ImportOptions::default(),
    ));
    cases.push((
        "GB_COORDINATE_OUT_OF_RANGE",
        simple_record_with_location("1..200"),
        ImportOptions::default(),
    ));
    cases.push((
        "MOLECULE_TYPE_UNKNOWN",
        fixture("162-unknown-molecule"),
        ImportOptions::default(),
    ));
    cases.push((
        "GB_UNKNOWN_FEATURE_KEY",
        fixture("070-unknown-key"),
        ImportOptions::default(),
    ));
    cases.push((
        "GB_QUALIFIER_UNQUOTED",
        record(
            "UNQ",
            "ATG",
            &[feature("misc_feature", "1..3", &["/note=bare"])],
            &[],
        ),
        ImportOptions::default(),
    ));
    cases.push((
        "GB_LOCATION_BETWEEN_POSITIONS",
        simple_record_with_location("100^101"),
        ImportOptions::default(),
    ));
    cases.push((
        "GB_LOCATION_PARTIAL_BOUNDARY",
        fixture("050-partial"),
        ImportOptions::default(),
    ));
    cases.push((
        "GB_FEATURE_COLUMN_MISALIGNED",
        format!("{}\nFEATURES             Location/Qualifiers\n  CDS{:16}1..3\nORIGIN\n        1 ATG\n//\n", locus("DRIFT", 3), "").into_bytes(),
        ImportOptions::default(),
    ));
    cases.push((
        "GB_SEQUENCE_NORMALIZED",
        record("NORM", "atg", &[], &[]),
        ImportOptions::default(),
    ));
    cases.push((
        "GB_CRLF_NORMALIZED",
        fixture("100-crlf"),
        ImportOptions::default(),
    ));
    cases.push((
        "GB_HEADER_FIELD_UNKNOWN",
        record("HDR", "ATG", &[], &[("SEGMENT", "1 of 2")]),
        ImportOptions::default(),
    ));
    cases.push((
        "RECORD_REQUIRED_FIELD_MISSING",
        fixture("160-missing-name"),
        ImportOptions::default(),
    ));
    let invalid_topology = format!(
        "{}\nORIGIN\n        1 ATG\n//\n",
        locus("TOPO", 3).replace("linear", "triangle")
    )
    .into_bytes();
    cases.push((
        "RECORD_FIELD_INVALID",
        invalid_topology,
        ImportOptions::default(),
    ));

    for (code, bytes, options) in cases {
        match import_genbank(&bytes, &options) {
            Ok(outcome) => {
                let diagnostic = outcome
                    .report
                    .diagnostics
                    .iter()
                    .find(|diagnostic| diagnostic.code == code)
                    .unwrap_or_else(|| panic!("code {code} missing from report"));
                assert_eq!(diagnostic.severity, severity_of(code), "severity of {code}");
            }
            Err(error) => {
                assert_eq!(error.code(), code, "expected code {code}");
                assert_eq!(
                    severity_of(code),
                    Severity::Error,
                    "error severity of {code}"
                );
            }
        }
    }
}
