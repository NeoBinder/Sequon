#![cfg(feature = "genbank")]

//! One test per GenBank fixture in plan section 7.2.
//!
//! Successful fixtures are compared byte for byte with the canonical JSON
//! golden. The goldens were generated from the reader after a field-by-field
//! review against docs/design/genbank-adapter.md; the targeted assertions below
//! are that review made executable.

use std::fs;
use std::path::PathBuf;

use sequon_io::api::{ImportOptions, ImportOutcome};
use sequon_io::import_genbank;
use sequon_io::json::to_canonical_json_bytes;
use sequon_io::report::Severity;
use sequon_io::ImportError;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata")
}

fn fixture(name: &str) -> Vec<u8> {
    fs::read(root().join("genbank").join(format!("{name}.gb"))).expect("fixture file")
}

fn golden(name: &str) -> Vec<u8> {
    fs::read(root().join("expected").join(format!("{name}.json"))).expect("golden file")
}

fn options(name: &str) -> ImportOptions {
    let mut options = ImportOptions::default();
    if name == "163-name-hint" {
        options.name_hint = Some("HINT163".to_string());
    }
    options
}

fn import(name: &str) -> ImportOutcome {
    import_genbank(&fixture(name), &options(name)).expect("fixture imports")
}

fn assert_golden(name: &str, outcome: &ImportOutcome) {
    assert_eq!(outcome.records.len(), 1, "{name} returns one record");
    assert_record_valid(&outcome.records[0]);
    let bytes = to_canonical_json_bytes(&outcome.records[0]).expect("canonical JSON");
    assert_eq!(bytes, golden(name), "{name} canonical JSON matches golden");
}

fn assert_record_valid(record: &sequon_model::Record) {
    record.validate().expect("record validates");
    sequon_algo::verify_digests(record).expect("digests verify");
}

fn has_diagnostic(outcome: &ImportOutcome, code: &str, severity: Severity) -> bool {
    outcome
        .report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == code && diagnostic.severity == severity)
}

fn error(name: &str) -> ImportError {
    import_genbank(&fixture(name), &options(name)).expect_err("fixture must fail")
}

#[test]
fn fixture_001_minimal_cds() {
    let outcome = import("001-minimal-cds");
    assert_golden("001-minimal-cds", &outcome);
    assert_eq!(outcome.records[0].length, 60);
    assert_eq!(outcome.records[0].features.len(), 2);
}

#[test]
fn fixture_010_circular_plasmid() {
    let outcome = import("010-circular-plasmid");
    assert_golden("010-circular-plasmid", &outcome);
    let record = &outcome.records[0];
    assert_eq!(record.topology, sequon_model::Topology::Circular);
    assert_eq!(record.features.len(), 4);
}

#[test]
fn fixture_020_join_complement() {
    let outcome = import("020-join-complement");
    assert_golden("020-join-complement", &outcome);
    let location = &outcome.records[0].features[0].location;
    assert_eq!(location.operator, sequon_model::OPERATOR_JOIN);
    assert_eq!(location.strand, -1);
    assert_eq!(location.segments.len(), 2);
    assert_eq!(
        (location.segments[0].start, location.segments[0].end),
        (9, 30)
    );
    assert_eq!(
        (location.segments[1].start, location.segments[1].end),
        (49, 70)
    );
}

#[test]
fn fixture_030_order() {
    let outcome = import("030-order");
    assert_golden("030-order", &outcome);
    let location = &outcome.records[0].features[0].location;
    assert_eq!(location.operator, sequon_model::OPERATOR_ORDER);
    assert_eq!(location.segments.len(), 3);
    assert_eq!(
        (location.segments[0].start, location.segments[0].end),
        (0, 10)
    );
    assert_eq!(
        (location.segments[2].start, location.segments[2].end),
        (0, 10)
    );
}

#[test]
fn fixture_040_near_origin() {
    let outcome = import("040-near-origin");
    assert_golden("040-near-origin", &outcome);
    let record = &outcome.records[0];
    let location = &record.features[0].location;
    assert_eq!(record.topology, sequon_model::Topology::Circular);
    assert_eq!(location.operator, sequon_model::OPERATOR_SPAN);
    assert_eq!(location.segments.len(), 1);
    assert_eq!(
        (location.segments[0].start, location.segments[0].end),
        (5899, 6100)
    );
    assert!(location.segments[0].is_wrapped(record.length));
}

#[test]
fn fixture_050_partial() {
    let outcome = import("050-partial");
    assert_golden("050-partial", &outcome);
    let cds = &outcome.records[0].features[0];
    assert!(cds.location.segments[0].start_partial);
    assert!(cds.location.segments[0].end_partial);
    let single = &outcome.records[0].features[1];
    assert!(single.location.segments[0].end_partial);
    assert!(has_diagnostic(
        &outcome,
        "GB_LOCATION_PARTIAL_BOUNDARY",
        Severity::Info
    ));
}

#[test]
fn fixture_060_duplicate_qualifiers() {
    let outcome = import("060-duplicate-qualifiers");
    assert_golden("060-duplicate-qualifiers", &outcome);
    let source = &outcome.records[0].features[0];
    assert_eq!(
        source.qualifiers[1].value.as_deref(),
        Some("first part of the note second part")
    );
    let feature = &outcome.records[0].features[1];
    let xrefs: Vec<&str> = feature
        .qualifiers
        .iter()
        .filter(|qualifier| qualifier.key == "db_xref")
        .filter_map(|qualifier| qualifier.value.as_deref())
        .collect();
    assert_eq!(xrefs, vec!["A:1", "B:2"]);
}

#[test]
fn fixture_070_unknown_key() {
    let outcome = import("070-unknown-key");
    assert_golden("070-unknown-key", &outcome);
    let feature = &outcome.records[0].features[0];
    assert_eq!(feature.key, "xyz_feature");
    assert_eq!(feature.so_term, None);
    assert!(has_diagnostic(
        &outcome,
        "GB_UNKNOWN_FEATURE_KEY",
        Severity::Info
    ));
}

#[test]
fn fixture_080_multi_record_all() {
    let outcome = import("080-multi-record");
    assert_eq!(outcome.records.len(), 2);
    for (index, record) in outcome.records.iter().enumerate() {
        assert_record_valid(record);
        let bytes = to_canonical_json_bytes(record).expect("canonical JSON");
        assert_eq!(
            bytes,
            golden(&format!("080-multi-record.{index}")),
            "record {index} golden"
        );
    }
    assert_eq!(outcome.records[0].name, "SYN080A");
    assert_eq!(outcome.records[1].name, "SYN080B");
    assert_eq!(outcome.report.records[1].index, 1);
}

#[test]
fn fixture_090_locus_length_mismatch() {
    let outcome = import("090-locus-length-mismatch");
    assert_golden("090-locus-length-mismatch", &outcome);
    assert_eq!(outcome.records[0].length, 60);
    assert!(has_diagnostic(
        &outcome,
        "GB_LOCUS_LENGTH_MISMATCH",
        Severity::Warning
    ));
}

#[test]
fn fixture_100_crlf() {
    let outcome = import("100-crlf");
    assert_golden("100-crlf", &outcome);
    assert!(has_diagnostic(
        &outcome,
        "GB_CRLF_NORMALIZED",
        Severity::Info
    ));
}

#[test]
fn fixture_110_no_features() {
    let outcome = import("110-no-features");
    assert_golden("110-no-features", &outcome);
    assert!(outcome.records[0].features.is_empty());
    let genbank = outcome.records[0]
        .extras
        .get("genbank")
        .expect("genbank extras");
    let object = genbank.as_object().expect("genbank object");
    assert!(
        !object.contains_key("division"),
        "blank division is omitted"
    );
}

#[test]
fn fixture_150_large_1mb() {
    let outcome = import("150-large-1mb");
    assert_golden("150-large-1mb", &outcome);
    assert_eq!(outcome.records[0].length, 1_000_000);
    assert_eq!(outcome.records[0].features.len(), 2);
}

#[test]
fn fixture_162_unknown_molecule() {
    let outcome = import("162-unknown-molecule");
    assert_golden("162-unknown-molecule", &outcome);
    assert_eq!(
        outcome.records[0].molecule_type,
        Some(sequon_model::MoleculeType::Other)
    );
    assert!(has_diagnostic(
        &outcome,
        "MOLECULE_TYPE_UNKNOWN",
        Severity::Warning
    ));
}

#[test]
fn fixture_163_name_hint() {
    let outcome = import("163-name-hint");
    assert_golden("163-name-hint", &outcome);
    assert_eq!(outcome.records[0].name, "HINT163");
}

#[test]
fn fixture_120_unterminated() {
    let error = error("120-unterminated");
    assert_eq!(error.code(), "GB_RECORD_UNTERMINATED");
    assert_eq!(
        error.position(),
        Some(&sequon_io::report::SourcePosition::Genbank {
            line: 7,
            column: None
        })
    );
}

#[test]
fn fixture_130_rna() {
    let error = error("130-rna");
    assert_eq!(error.code(), "GB_UNSUPPORTED_MOLECULE");
    assert_eq!(
        error.position(),
        Some(&sequon_io::report::SourcePosition::Genbank {
            line: 1,
            column: None
        })
    );
}

#[test]
fn fixture_140_bad_location() {
    let error = error("140-bad-location");
    assert_eq!(error.code(), "GB_LOCATION_MALFORMED");
    assert_eq!(
        error.position(),
        Some(&sequon_io::report::SourcePosition::Genbank {
            line: 4,
            column: Some(25)
        })
    );
}

#[test]
fn fixture_160_missing_name() {
    let error = error("160-missing-name");
    assert_eq!(error.code(), "RECORD_REQUIRED_FIELD_MISSING");
    assert!(error.to_string().contains("name"));
}

#[test]
fn fixture_161_missing_topology() {
    let error = error("161-missing-topology");
    assert_eq!(error.code(), "GB_TOPOLOGY_MISSING");
}
