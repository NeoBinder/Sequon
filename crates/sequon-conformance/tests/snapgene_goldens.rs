//! Canonical JSON golden gate for the successful SnapGene fixtures.
//!
//! A golden is the canonical JSON text of the record produced by the committed
//! M4 reader (see `testdata/README.md` section 6 and
//! `src/bin/gen-snapgene-goldens.rs`). This test enforces:
//!
//! - every successful fixture re-imports to exactly one record;
//! - the record passes `Record::validate()` and
//!   `sequon_algo::verify_digests()`;
//! - re-serializing it byte-equals the committed golden;
//! - the golden parses back into a valid record with verified digests (full
//!   JSON Schema validation of every golden runs in `tools/validate_contract.py`);
//! - every failing fixture returns its documented typed error code and has no
//!   golden.
//!
//! A mismatch is a reader or golden change to review deliberately; it is never
//! auto-accepted. A failing fixture whose error differs is a parser bug to
//! escalate, not a golden to accept.

use std::fs;
use std::path::{Path, PathBuf};

use sequon_io::api::{Format, ImportOptions};
use sequon_io::report::Severity;
use serde_json::Value;

use sequon_conformance::snapgene_fixtures::{
    expected_diagnostics, expected_error, failing, successful, Fixture,
};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate lives at <workspace>/crates/sequon-conformance")
        .to_path_buf()
}

fn fixture_directory() -> PathBuf {
    workspace_root().join("testdata/snapgene")
}

fn golden_directory() -> PathBuf {
    workspace_root().join("testdata/expected")
}

fn stem(fixture_name: &str) -> &str {
    fixture_name.strip_suffix(".dna").unwrap_or(fixture_name)
}

fn golden_path(fixture: &Fixture) -> PathBuf {
    golden_directory().join(format!("{}.json", stem(fixture.name)))
}

fn read_fixture(fixture: &Fixture) -> Vec<u8> {
    fs::read(fixture_directory().join(fixture.name))
        .unwrap_or_else(|error| panic!("read {}: {error}", fixture.name))
}

fn import(bytes: &[u8]) -> sequon_io::api::ImportResult {
    let options = ImportOptions {
        format: Some(Format::Snapgene),
        ..ImportOptions::default()
    };
    sequon_io::import_snapgene(bytes, &options)
}

#[test]
fn goldens_match_the_reader_bytes() {
    for fixture in successful() {
        let outcome = import(&read_fixture(&fixture)).unwrap_or_else(|error| {
            panic!(
                "{} must import (reader bug, not a golden change): {error}",
                fixture.name
            )
        });
        assert_eq!(outcome.records.len(), 1, "{} record count", fixture.name);
        let record = &outcome.records[0];
        record
            .validate()
            .unwrap_or_else(|error| panic!("{} record invalid: {error}", fixture.name));
        sequon_algo::verify_digests(record)
            .unwrap_or_else(|error| panic!("{} digests do not verify: {error}", fixture.name));

        assert!(
            outcome
                .report
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.severity != Severity::Error),
            "{} returned an error-severity diagnostic",
            fixture.name
        );
        for code in expected_diagnostics(fixture.name) {
            assert!(
                outcome
                    .report
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == *code),
                "{} is missing expected diagnostic {code}",
                fixture.name
            );
        }

        let rendered = sequon_io::json::to_canonical_json_bytes(record)
            .unwrap_or_else(|error| panic!("{} canonical JSON: {error}", fixture.name));
        let path = golden_path(&fixture);
        let committed =
            fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        assert_eq!(
            committed,
            rendered,
            "{} golden differs from the current reader output ({} committed vs {} rebuilt bytes); review the diff and regenerate deliberately with gen-snapgene-goldens",
            stem(fixture.name),
            committed.len(),
            rendered.len()
        );
    }
}

#[test]
fn failing_fixtures_have_their_documented_error_and_no_golden() {
    for fixture in failing() {
        let expected = expected_error(fixture.name)
            .unwrap_or_else(|| panic!("{} is in the failing set without a code", fixture.name));
        let error = match import(&read_fixture(&fixture)) {
            Ok(_) => panic!(
                "{} must fail with {expected} (a success is a parser bug to escalate)",
                fixture.name
            ),
            Err(error) => error,
        };
        assert_eq!(error.code(), expected, "{} error code", fixture.name);
        assert!(
            !golden_path(&fixture).exists(),
            "{} must not have a golden",
            fixture.name
        );
    }
}

#[test]
fn goldens_parse_validate_and_verify_digests() {
    for fixture in successful() {
        let path = golden_path(&fixture);
        let bytes =
            fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        let record = sequon_io::json::parse_canonical_json(&bytes)
            .unwrap_or_else(|error| panic!("parse {} as record: {error}", path.display()));
        record
            .validate()
            .unwrap_or_else(|error| panic!("{} golden is invalid: {error}", path.display()));
        sequon_algo::verify_digests(&record)
            .unwrap_or_else(|error| panic!("{} golden digests: {error}", path.display()));
    }
}
/// Deliberate golden regeneration. Ignored by default and gated on an
/// environment variable so CI can never rewrite goldens: set
/// @@BT@@SEQUON_REGEN_SNAPGENE_GOLDENS=1@@BT@@ and run this test explicitly. A
/// regeneration is a reviewed change that must explain why reader output changed.
#[test]
#[ignore = "writes testdata/expected; set SEQUON_REGEN_SNAPGENE_GOLDENS=1 and run explicitly"]
fn regenerate_goldens() {
    if std::env::var("SEQUON_REGEN_SNAPGENE_GOLDENS").as_deref() != Ok("1") {
        eprintln!("SEQUON_REGEN_SNAPGENE_GOLDENS is not 1; nothing written");
        return;
    }
    fs::create_dir_all(golden_directory()).expect("create testdata/expected");
    for fixture in successful() {
        let outcome = import(&read_fixture(&fixture))
            .unwrap_or_else(|error| panic!("{} must import: {error}", fixture.name));
        assert_eq!(outcome.records.len(), 1, "{} record count", fixture.name);
        let record = &outcome.records[0];
        record
            .validate()
            .unwrap_or_else(|error| panic!("{} record invalid: {error}", fixture.name));
        sequon_algo::verify_digests(record)
            .unwrap_or_else(|error| panic!("{} digests: {error}", fixture.name));
        let bytes = sequon_io::json::to_canonical_json_bytes(record)
            .unwrap_or_else(|error| panic!("{} canonical JSON: {error}", fixture.name));
        let path = golden_path(&fixture);
        fs::write(&path, &bytes)
            .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
        println!("wrote {} ({} bytes)", path.display(), bytes.len());
    }
}
/// The type-1 golden and the type-0 golden must describe the same molecule.
///
/// This is the paired check the design note (F-8..F-11) requires: if the 2-bit
/// decoder is wrong, the sequences diverge even though each import "succeeds".
#[test]
fn two_bit_twin_matches_the_type0_golden() {
    let type0: Value = serde_json::from_slice(
        &fs::read(golden_directory().join("001-linear-uncompressed.json")).expect("type-0 golden"),
    )
    .expect("type-0 golden JSON");
    let type1: Value = serde_json::from_slice(
        &fs::read(golden_directory().join("002-circular-compressed.json")).expect("type-1 golden"),
    )
    .expect("type-1 golden JSON");
    assert_eq!(
        type0["sequence"], type1["sequence"],
        "the 2-bit twin must decode to the same sequence"
    );
}
