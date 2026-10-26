//! Conformance comparator tests (deliverable C9).
//!
//! The synthetic pair is produced entirely in this test: a `.dna` container is
//! built with the conformance builder, imported, written back with the GenBank
//! writer and imported again. The writer round-trip must compare as identical.
//! Negative cases mutate one dimension at a time so each threshold and category
//! is exercised. The local pET-28a(+) pair is gated on the gitignored corpus
//! files being present.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use sequon_conformance::compare::{
    compare_records, CompareOptions, CompareReport, DifferenceCategory,
};
use sequon_conformance::dna_builder::{ContainerBuilder, Feature, Notes, Segment};
use sequon_io::api::{Format, ImportOptions, WriteOptions};
use sequon_model::{Location, Record, Segment as ModelSegment, Topology};

/// 120 bp self-authored sequence for the synthetic pair.
const SEQUENCE: &str = "ACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGT";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate lives at <workspace>/crates/sequon-conformance")
        .to_path_buf()
}

fn scratch(tag: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("sequon-compare-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("create scratch directory");
    directory
}

fn import_one(format: Format, bytes: &[u8], label: &str) -> Record {
    let options = ImportOptions {
        format: Some(format),
        ..ImportOptions::default()
    };
    let outcome = match format {
        Format::Snapgene => sequon_io::import_snapgene(bytes, &options),
        Format::Genbank => sequon_io::import_genbank(bytes, &options),
    }
    .unwrap_or_else(|error| panic!("{label} import: {error}"));
    assert_eq!(outcome.records.len(), 1, "{label} record count");
    let record = &outcome.records[0];
    record
        .validate()
        .unwrap_or_else(|error| panic!("{label} validation: {error}"));
    sequon_algo::verify_digests(record).unwrap_or_else(|error| panic!("{label} digests: {error}"));
    record.clone()
}

fn import_gb_file(path: &Path) -> Record {
    let bytes = fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    import_one(Format::Genbank, &bytes, "gb")
}

fn write_gb_file(record: &Record, path: &Path) {
    let outcome = sequon_io::write_genbank(record, &WriteOptions::default())
        .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    fs::write(path, &outcome.bytes)
        .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
}

/// Builds the synthetic `.dna` container used by the round-trip and negative tests.
fn build_dna_bytes() -> Vec<u8> {
    let features = vec![
        Feature::new("CDS", vec![Segment::new(10, 40)])
            .name("synthetic cds")
            .directionality(2)
            .text_qualifier("gene", "synthetic")
            .int_qualifier("codon_start", 1)
            .text_qualifier("note", "synthetic CDS note"),
        Feature::new("misc_feature", vec![Segment::new(1, 8)])
            .text_qualifier("note", "synthetic site"),
    ];
    let mut notes = Notes::new();
    notes.field("CustomMapLabel", "compare-synthetic");
    notes.field("UseCustomMapLabel", "1");
    notes.field("Organism", "synthetic construct");
    let mut container = ContainerBuilder::new(1, 15);
    container
        .push_dna(0x01, SEQUENCE)
        .push_notes(&notes)
        .push_features(&features);
    container.build()
}

fn synthetic_record() -> Record {
    import_one(Format::Snapgene, &build_dna_bytes(), "dna")
}

fn assert_category(report: &CompareReport, category: DifferenceCategory) {
    assert!(
        report
            .differences
            .iter()
            .any(|difference| difference.category == category && difference.threshold_failure),
        "expected a threshold-failing {category:?} difference, got {:#?}",
        report.differences
    );
}

#[test]
fn synthetic_writer_round_trip_passes_all_thresholds() {
    let directory = scratch("round-trip");
    let dna = synthetic_record();
    let gb_path = directory.join("synthetic.gb");
    write_gb_file(&dna, &gb_path);
    let gb = import_gb_file(&gb_path);
    let report = compare_records(&dna, &gb, &CompareOptions::default());

    assert!(report.passed, "round-trip report: {report:#?}");
    assert!(report.thresholds.passed);
    assert_eq!(report.features.jaccard, 1.0);
    assert!(report.features.segment_set.same);
    assert!(report.qualifiers.differences.is_empty());
    assert_eq!(report.primers.dna_count, 0);
    assert_eq!(report.sequence.canonical_digest_equal, Some(true));
    assert_eq!(report.features.matched, 2);
    assert!(report.differences.is_empty());
}

#[test]
fn one_base_sequence_change_fails_the_sequence_threshold() {
    let directory = scratch("sequence");
    let dna = synthetic_record();
    let mut mutated = dna.clone();
    let mut bases: Vec<char> = mutated.sequence.chars().collect();
    bases[0] = if bases[0] == 'A' { 'C' } else { 'A' };
    mutated.sequence = bases.into_iter().collect();
    let gb_path = directory.join("mutated.gb");
    write_gb_file(&mutated, &gb_path);
    let gb = import_gb_file(&gb_path);

    let report = compare_records(&dna, &gb, &CompareOptions::default());
    assert!(!report.passed);
    assert!(!report.thresholds.sequence);
    assert!(!report.sequence.normalized_equal);
    assert!(!report.sequence.sequence_digest_equal);
    assert_category(&report, DifferenceCategory::Sequence);
}

#[test]
fn topology_mismatch_fails_topology_and_skips_canonical_digest() {
    let directory = scratch("topology");
    let dna = synthetic_record();
    let mut mutated = dna.clone();
    mutated.topology = Topology::Linear;
    let gb_path = directory.join("linear.gb");
    write_gb_file(&mutated, &gb_path);
    let gb = import_gb_file(&gb_path);

    let report = compare_records(&dna, &gb, &CompareOptions::default());
    assert!(!report.passed);
    assert!(!report.thresholds.topology);
    assert_eq!(report.sequence.canonical_digest_equal, None);
    assert_category(&report, DifferenceCategory::Topology);
}

#[test]
fn removed_feature_fails_the_feature_jaccard() {
    let directory = scratch("features");
    let dna = synthetic_record();
    let mut mutated = dna.clone();
    mutated.features.pop();
    let gb_path = directory.join("fewer-features.gb");
    write_gb_file(&mutated, &gb_path);
    let gb = import_gb_file(&gb_path);

    let report = compare_records(&dna, &gb, &CompareOptions::default());
    assert!(!report.passed);
    assert!(!report.thresholds.features);
    assert!(report.features.jaccard < 0.99);
    assert_eq!(report.features.missing.len(), 1);
    assert_category(&report, DifferenceCategory::FeatureSet);
}

#[test]
fn changed_qualifier_fails_the_qualifier_threshold() {
    let directory = scratch("qualifier");
    let dna = synthetic_record();
    let mut mutated = dna.clone();
    mutated.features[0].qualifiers[0].value = Some("changed value".to_owned());
    let gb_path = directory.join("qualifier.gb");
    write_gb_file(&mutated, &gb_path);
    let gb = import_gb_file(&gb_path);

    let report = compare_records(&dna, &gb, &CompareOptions::default());
    assert!(!report.passed);
    assert!(!report.thresholds.qualifiers);
    assert!(!report.qualifiers.differences.is_empty());
    assert_category(&report, DifferenceCategory::Qualifier);
}

#[test]
fn local_pet28a_pair_reports_sequence_match_and_feature_gap() {
    let directory = workspace_root().join("testdata/corpus-local/pet28a-pair");
    let dna_path = directory.join("pET-28a(+).dna");
    let gb_path = directory.join("pET-28a(+)-PU845465-circular.gb");
    if !dna_path.is_file() || !gb_path.is_file() {
        eprintln!("local pET-28a(+) pair is absent; comparison skipped");
        return;
    }

    let dna_bytes = fs::read(&dna_path).expect("read local .dna");
    let dna = import_one(Format::Snapgene, &dna_bytes, "local dna");
    let gb = import_gb_file(&gb_path);
    let report = compare_records(&dna, &gb, &CompareOptions::default());

    println!(
        "local pair: sequence_pass={} topology_pass={} jaccard={:.4} ({}/{}) segment_jaccard={:.4} primers={}/{}",
        report.thresholds.sequence,
        report.thresholds.topology,
        report.features.jaccard,
        report.features.matched,
        report.features.union,
        report.features.segment_set.jaccard,
        report.primers.dna_count,
        report.primers.gb_count
    );

    assert!(report.sequence.lengths_equal);
    assert!(report.sequence.normalized_equal);
    assert!(report.sequence.sequence_digest_equal);
    assert!(report.topology.equal);
    assert_eq!(report.sequence.canonical_digest_equal, Some(true));
    assert!(report.thresholds.sequence);
    assert!(report.thresholds.topology);

    // The GenBank file is a patent sequence record with two features, not a
    // SnapGene Save-As export, so the feature gate is expected to fail. It must
    // be reported as a failure, not silently ignored.
    assert!(!report.features.passed);
    assert!(!report.features.missing.is_empty());
    assert!(!report.passed);
}

fn run_cli(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sequon-conformance"))
        .args(arguments)
        .output()
        .expect("run conformance binary")
}

#[test]
fn cli_passes_synthetic_pair_and_writes_json() {
    let directory = scratch("cli-pass");
    let dna_path = directory.join("pair.dna");
    let gb_path = directory.join("pair.gb");
    let json_path = directory.join("report.json");
    fs::write(&dna_path, build_dna_bytes()).expect("write dna fixture");
    let dna = import_one(Format::Snapgene, &build_dna_bytes(), "dna");
    write_gb_file(&dna, &gb_path);

    let output = run_cli(&[
        "compare",
        "--dna",
        dna_path.to_str().expect("path"),
        "--gb",
        gb_path.to_str().expect("path"),
        "--json",
        json_path.to_str().expect("path"),
    ]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("RESULT: PASS"), "stdout: {stdout}");

    let json = fs::read_to_string(&json_path).expect("read report json");
    let document: serde_json::Value = serde_json::from_str(&json).expect("report is JSON");
    assert_eq!(document["schema"], "sequon/conformance-compare/1");
    assert_eq!(document["dna"], "pair.dna");
    assert_eq!(document["report"]["passed"], true);
    assert_eq!(document["report"]["thresholds"]["features"], true);
}

#[test]
fn cli_reports_mismatch_with_exit_code_one() {
    let directory = scratch("cli-fail");
    let dna_path = directory.join("pair.dna");
    let gb_path = directory.join("pair.gb");
    fs::write(&dna_path, build_dna_bytes()).expect("write dna fixture");
    let mut mutated = synthetic_record();
    let mut bases: Vec<char> = mutated.sequence.chars().collect();
    bases[0] = if bases[0] == 'A' { 'C' } else { 'A' };
    mutated.sequence = bases.into_iter().collect();
    write_gb_file(&mutated, &gb_path);

    let output = run_cli(&[
        "compare",
        "--dna",
        dna_path.to_str().expect("path"),
        "--gb",
        gb_path.to_str().expect("path"),
    ]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("RESULT: FAIL"), "stdout: {stdout}");
}

#[test]
fn cli_usage_and_multi_record_errors_exit_code_two() {
    let directory = scratch("cli-usage");
    let dna_path = directory.join("pair.dna");
    let gb_path = directory.join("pair.gb");
    let multi_path = directory.join("multi.gb");
    fs::write(&dna_path, build_dna_bytes()).expect("write dna fixture");
    let dna = import_one(Format::Snapgene, &build_dna_bytes(), "dna");
    write_gb_file(&dna, &gb_path);
    let single = fs::read(&gb_path).expect("read gb");
    let mut multi = single.clone();
    multi.extend_from_slice(&single);
    fs::write(&multi_path, multi).expect("write multi-record gb");

    let missing_flag = run_cli(&["compare", "--dna", dna_path.to_str().expect("path")]);
    assert_eq!(missing_flag.status.code(), Some(2));

    let unknown = run_cli(&["frobnicate"]);
    assert_eq!(unknown.status.code(), Some(2));

    let bad_threshold = run_cli(&[
        "compare",
        "--dna",
        dna_path.to_str().expect("path"),
        "--gb",
        gb_path.to_str().expect("path"),
        "--feature-threshold",
        "1.5",
    ]);
    assert_eq!(bad_threshold.status.code(), Some(2));

    let multi_record = run_cli(&[
        "compare",
        "--dna",
        dna_path.to_str().expect("path"),
        "--gb",
        multi_path.to_str().expect("path"),
    ]);
    assert_eq!(
        multi_record.status.code(),
        Some(2),
        "stderr: {}",
        String::from_utf8_lossy(&multi_record.stderr)
    );
}

#[test]
fn duplicate_feature_is_reported_instead_of_cancelling_out() {
    // Set semantics used to let the extra duplicate cancel out against the
    // single counterpart, producing a false pass with exit code 0.
    let dna = synthetic_record();
    let mut gb = dna.clone();
    gb.features.push(dna.features[0].clone());

    let report = compare_records(&dna, &gb, &CompareOptions::default());
    assert!(!report.passed);
    assert!(!report.thresholds.features);
    assert_eq!(report.features.matched, 2);
    assert_eq!(report.features.union, 3);
    assert_eq!(report.features.extra.len(), 1);
    assert!(report.features.missing.is_empty());
    assert_category(&report, DifferenceCategory::FeatureSet);
}

#[test]
fn quoted_qualifier_round_trip_passes_after_normalization() {
    // The GenBank writer replaces an embedded double quote with two apostrophes
    // and reports WRITE_QUOTE_NORMALIZED. The documented normalization reverses
    // that escape, so a written-then-read pair must not show a qualifier
    // difference.
    let directory = scratch("quote-normalization");
    let mut dna = synthetic_record();
    dna.features[0].qualifiers[0].value = Some("say \"hi\"".to_owned());
    let gb_path = directory.join("quoted.gb");
    write_gb_file(&dna, &gb_path);
    let gb = import_gb_file(&gb_path);

    let report = compare_records(&dna, &gb, &CompareOptions::default());
    assert!(report.passed, "round-trip report: {report:#?}");
}

#[test]
fn primer_count_mismatch_fails_the_primer_threshold() {
    let dna = synthetic_record();
    let mut gb = dna.clone();
    let mut primer = dna.features[0].clone();
    primer.key = "primer_bind".to_owned();
    gb.features.push(primer);

    let report = compare_records(&dna, &gb, &CompareOptions::default());
    assert!(!report.primers.passed);
    assert!(!report.thresholds.primers);
    assert_eq!(report.primers.dna_count, 0);
    assert_eq!(report.primers.gb_count, 1);
}

#[test]
fn empty_feature_sets_pass_the_feature_threshold() {
    let mut dna = synthetic_record();
    dna.features.clear();
    let gb = dna.clone();

    let report = compare_records(&dna, &gb, &CompareOptions::default());
    assert!(report.passed);
    assert_eq!(report.features.matched, 0);
    assert_eq!(report.features.union, 0);
    assert_eq!(report.features.jaccard, 1.0);
    assert!(report.thresholds.features);
}

#[test]
fn wrapped_circular_span_matches_the_origin_order_preserving_split() {
    // A SnapGene range that crosses the origin is stored as one unwrapped span;
    // the GenBank writer spells it as join(tail, head). Both must resolve to the
    // same coarse tuple and the same segment set.
    let mut dna = synthetic_record();
    dna.topology = Topology::Circular;
    let mut wrapped = dna.features[0].clone();
    wrapped.location = Location::span(1, 110, 130);
    dna.features = vec![wrapped];

    let mut gb = dna.clone();
    gb.features[0].location = Location::join(
        1,
        vec![ModelSegment::new(110, 120), ModelSegment::new(0, 10)],
    );

    let report = compare_records(&dna, &gb, &CompareOptions::default());
    assert!(report.passed, "wrapped report: {report:#?}");
    assert_eq!(report.features.matched, 1);
    assert_eq!(report.features.jaccard, 1.0);
    assert!(report.features.segment_set.same);
}
