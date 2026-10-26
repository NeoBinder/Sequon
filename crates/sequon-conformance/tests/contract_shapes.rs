//! Rust-side contract shape checks.
//!
//! Full JSON Schema validation runs in `tests/schema_validation.rs` (the
//! `jsonschema` crate is a dev-dependency of this dev-only harness). These tests
//! add the serde/model-level guarantees that do not need a schema engine:
//! the schema files still declare the expected identity and required fields,
//! the committed examples parse into the typed model and validate, and the
//! typed parsers reject unknown envelope fields and floating-point numbers.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use sequon_io::json::{parse_canonical_json, parse_import_report};

const RECORD_SCHEMA_ID: &str = "sequon/record/1";
const REPORT_SCHEMA_ID: &str = "sequon/import-report/1";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate lives at <workspace>/crates/sequon-conformance")
        .to_path_buf()
}

fn read_json(path: &Path) -> Value {
    let bytes = fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}

fn example(schema_id: &str) -> (PathBuf, Value) {
    let directory = workspace_root().join("schemas/examples");
    let mut paths: Vec<PathBuf> = fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()))
        .map(|entry| entry.expect("readable entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    paths.sort();
    for path in paths {
        let document = read_json(&path);
        if document.get("schema").and_then(Value::as_str) == Some(schema_id) {
            return (path, document);
        }
    }
    panic!("schemas/examples must contain a {schema_id} example");
}

#[test]
fn schemas_declare_the_expected_contract_identity() {
    let root = workspace_root();
    let record = read_json(&root.join("schemas/sequon-record-1.schema.json"));
    let report = read_json(&root.join("schemas/import-report-1.schema.json"));

    assert_eq!(
        record.get("$schema").and_then(Value::as_str),
        Some("https://json-schema.org/draft/2020-12/schema")
    );
    assert_eq!(
        report.get("$schema").and_then(Value::as_str),
        Some("https://json-schema.org/draft/2020-12/schema")
    );
    assert_eq!(
        record.get("$id").and_then(Value::as_str),
        Some("urn:sequon:schema:record:1")
    );
    assert_eq!(
        report.get("$id").and_then(Value::as_str),
        Some("urn:sequon:schema:import-report:1")
    );

    let required = |schema: &Value| -> Vec<String> {
        schema
            .get("required")
            .and_then(Value::as_array)
            .expect("schema declares required fields")
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .expect("required entry is a string")
                    .to_owned()
            })
            .collect()
    };
    for field in [
        "schema",
        "kind",
        "topology",
        "name",
        "length",
        "sequence",
        "digests",
        "features",
        "provenance",
        "extras",
    ] {
        assert!(
            required(&record).iter().any(|entry| entry == field),
            "record schema must require {field}"
        );
    }
    assert!(
        required(&report)
            .iter()
            .any(|entry| entry == "codes_version"),
        "report schema must require codes_version"
    );
    assert_eq!(
        record.get("additionalProperties").and_then(Value::as_bool),
        Some(false)
    );
}

#[test]
fn examples_parse_into_the_typed_model() {
    let (record_path, _) = example(RECORD_SCHEMA_ID);
    let record_bytes = fs::read(&record_path).expect("read record example");
    let record = parse_canonical_json(&record_bytes).expect("record example parses");
    record
        .validate()
        .expect("record example passes Record::validate()");
    sequon_algo::verify_digests(&record).expect("record example digests verify");

    let (report_path, _) = example(REPORT_SCHEMA_ID);
    let report_bytes = fs::read(&report_path).expect("read report example");
    let report = parse_import_report(&report_bytes).expect("report example parses");
    assert_eq!(report.schema, REPORT_SCHEMA_ID);
    assert_eq!(report.records.len(), 1);
}

#[test]
fn typed_parsers_reject_unknown_fields_and_floats() {
    let (_, record) = example(RECORD_SCHEMA_ID);

    let mut unknown = record.clone();
    unknown["unexpected_field"] = json!(true);
    assert!(
        serde_json::from_value::<sequon_model::Record>(unknown).is_err(),
        "unknown envelope fields must be rejected by the typed model"
    );

    let mut float = record;
    float["extras"] = json!({"example": {"value": 1.5}});
    assert!(
        serde_json::from_value::<sequon_model::Record>(float).is_err(),
        "floating-point numbers in extras must be rejected"
    );
}

#[test]
fn contract_files_are_utf8_lf_with_one_final_newline() {
    let root = workspace_root();
    let mut files: Vec<PathBuf> = Vec::new();
    for directory in ["schemas", "schemas/examples", "testdata/expected"] {
        let path = root.join(directory);
        if !path.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&path).expect("read directory") {
            let entry = entry.expect("readable entry").path();
            if entry
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                files.push(entry);
            }
        }
    }
    assert!(
        !files.is_empty(),
        "there must be contract JSON files to check"
    );
    let schema_directory = root.join("schemas");
    for path in files {
        let bytes = fs::read(&path).expect("read contract file");
        std::str::from_utf8(&bytes)
            .unwrap_or_else(|error| panic!("{} must be valid UTF-8: {error}", path.display()));
        if path.parent() == Some(schema_directory.as_path()) {
            assert!(
                bytes.is_ascii(),
                "{} schema definition must stay ASCII",
                path.display()
            );
        }
        assert!(
            !bytes.contains(&b'\r'),
            "{} must use LF line endings",
            path.display()
        );
        assert!(
            bytes.ends_with(b"\n") && !bytes.ends_with(b"\n\n"),
            "{} must end with exactly one newline",
            path.display()
        );
    }
}
