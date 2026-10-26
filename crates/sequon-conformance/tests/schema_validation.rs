//! JSON Schema validation for the Sequon contract.
//!
//! Every document in `schemas/examples/` must validate against the schema named
//! by its `schema` field, and every committed golden in `testdata/expected/`
//! must validate against the record schema. The negative cases below prove that
//! the schemas reject malformed documents; they are a contract check, not an
//! adapter test.
//!
//! Cross-field invariants that JSON Schema cannot express are intentionally
//! absent here and must be covered by `Record::validate()` (M2) and adapter
//! tests later:
//!
//! - `0 <= start <= end <= length` for every segment (the schema cannot compare
//!   a segment against the record `length`);
//! - `length == sequence.chars().count()`;
//! - `digests.sequence` and `digests.canonical` equal the recomputed sha256
//!   values;
//! - report `records[].length`, `sequence_digest` and `canonical_digest` agree
//!   with the corresponding record;
//! - `dropped` and `preserved` never describe the same byte range;
//! - `preserved.data` is actually valid base64: JSON Schema 2020-12 treats
//!   `contentEncoding` as an annotation and does not decode it (pinned below);
//! - the JSON profile rules (field order, no floating-point numbers, LF and a
//!   trailing newline): `"type": "integer"` even accepts `1.0`, so these are
//!   serializer and `Record::validate()` rules, not schema rules.
//!
//! The schemas are owned by the contract role; this test never edits them.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

const RECORD_SCHEMA_ID: &str = "sequon/record/1";
const REPORT_SCHEMA_ID: &str = "sequon/import-report/1";
const RECORD_SCHEMA_FILE: &str = "schemas/sequon-record-1.schema.json";
const REPORT_SCHEMA_FILE: &str = "schemas/import-report-1.schema.json";

/// Workspace root, derived from this crate's location.
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

fn compiled_schema(relative: &str) -> jsonschema::Validator {
    let path = workspace_root().join(relative);
    let schema = read_json(&path);
    assert_eq!(
        schema.get("$schema").and_then(Value::as_str),
        Some("https://json-schema.org/draft/2020-12/schema"),
        "{} must declare the draft 2020-12 meta-schema",
        path.display()
    );
    jsonschema::meta::validate(&schema).unwrap_or_else(|error| {
        panic!(
            "schema {} must validate against its meta-schema: {error}",
            path.display()
        )
    });
    jsonschema::validator_for(&schema)
        .unwrap_or_else(|error| panic!("schema {} must compile: {error}", path.display()))
}

fn validation_errors(validator: &jsonschema::Validator, instance: &Value) -> Vec<String> {
    validator
        .iter_errors(instance)
        .map(|error| format!("{error} at {}", error.instance_path()))
        .collect()
}

/// All `schemas/examples/*.json` documents, sorted by path for stable output.
fn example_documents() -> Vec<(PathBuf, Value)> {
    let directory = workspace_root().join("schemas/examples");
    let entries = fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("read examples dir {}: {error}", directory.display()));
    let mut documents: Vec<(PathBuf, Value)> = entries
        .map(|entry| entry.expect("readable directory entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .map(|path| {
            let document = read_json(&path);
            (path, document)
        })
        .collect();
    documents.sort_by(|left, right| left.0.cmp(&right.0));
    assert!(
        !documents.is_empty(),
        "schemas/examples/ must contain at least one example per schema"
    );
    documents
}

fn example_for(schema_id: &str) -> Value {
    example_documents()
        .into_iter()
        .find(|(_, document)| document.get("schema").and_then(Value::as_str) == Some(schema_id))
        .map(|(_, document)| document)
        .unwrap_or_else(|| panic!("schemas/examples/ must contain a {schema_id} example"))
}

fn assert_rejected(validator: &jsonschema::Validator, instance: &Value, case: &str) {
    assert!(
        !validator.is_valid(instance),
        "negative case must be rejected by the schema: {case}"
    );
}

fn segment(start: i64, end: i64) -> Value {
    json!({
        "start": start,
        "end": end,
        "start_partial": false,
        "end_partial": false,
    })
}

/// Append one feature with the given location to a valid record example.
fn record_with_location(record: &Value, location: Value) -> Value {
    let mut document = record.clone();
    let features = document
        .get_mut("features")
        .and_then(Value::as_array_mut)
        .expect("record example has a features array");
    features.push(json!({
        "key": "misc_feature",
        "location": location,
        "qualifiers": [],
    }));
    document
}

#[test]
fn examples_validate_against_their_schema() {
    let record_validator = compiled_schema(RECORD_SCHEMA_FILE);
    let report_validator = compiled_schema(REPORT_SCHEMA_FILE);
    let mut records = 0_usize;
    let mut reports = 0_usize;

    for (path, document) in example_documents() {
        let validator = match document.get("schema").and_then(Value::as_str) {
            Some(RECORD_SCHEMA_ID) => {
                records += 1;
                &record_validator
            }
            Some(REPORT_SCHEMA_ID) => {
                reports += 1;
                &report_validator
            }
            other => panic!("{}: unsupported schema marker {other:?}", path.display()),
        };
        let errors = validation_errors(validator, &document);
        assert!(
            errors.is_empty(),
            "{} failed schema validation:\n{}",
            path.display(),
            errors.join("\n")
        );
    }

    assert!(records > 0, "no {RECORD_SCHEMA_ID} example validated");
    assert!(reports > 0, "no {REPORT_SCHEMA_ID} example validated");
}

#[test]
fn record_schema_rejects_malformed_documents() {
    let validator = compiled_schema(RECORD_SCHEMA_FILE);
    let base = example_for(RECORD_SCHEMA_ID);

    let mut missing_required = base.clone();
    missing_required
        .as_object_mut()
        .expect("record example is an object")
        .remove("sequence");
    assert_rejected(
        &validator,
        &missing_required,
        "missing required property: sequence",
    );

    let mut zero_length = base.clone();
    zero_length["length"] = json!(0);
    assert_rejected(&validator, &zero_length, "length below the minimum of 1");

    let mut negative_start = base.clone();
    negative_start = record_with_location(
        &negative_start,
        json!({"operator": "span", "strand": 1, "segments": [segment(-1, 4)]}),
    );
    assert_rejected(&validator, &negative_start, "negative segment start");

    let two_segment_span = record_with_location(
        &base,
        json!({"operator": "span", "strand": 1, "segments": [segment(0, 4), segment(8, 12)]}),
    );
    assert_rejected(
        &validator,
        &two_segment_span,
        "span operator declared with two segments",
    );

    let mut bad_digest = base.clone();
    bad_digest["digests"]["sequence"] = json!("sha256:NOT-A-DIGEST");
    assert_rejected(&validator, &bad_digest, "malformed sha256 digest");

    let mut unknown_kind = base.clone();
    unknown_kind["kind"] = json!("plasmid");
    assert_rejected(&validator, &unknown_kind, "kind outside the enum");

    let mut unknown_property = base.clone();
    unknown_property["unexpected_field"] = json!(true);
    assert_rejected(&validator, &unknown_property, "unknown top-level property");
}

#[test]
fn report_schema_rejects_malformed_documents() {
    let validator = compiled_schema(REPORT_SCHEMA_FILE);
    let base = example_for(REPORT_SCHEMA_ID);

    let mut missing_required = base.clone();
    missing_required
        .as_object_mut()
        .expect("report example is an object")
        .remove("diagnostics");
    assert_rejected(
        &validator,
        &missing_required,
        "missing required property: diagnostics",
    );

    let mut negative_bytes = base.clone();
    negative_bytes["source_bytes"] = json!(-1);
    assert_rejected(&validator, &negative_bytes, "negative source_bytes");

    let mut bad_digest = base.clone();
    bad_digest["source_digest"] = json!("sha256:ABC");
    assert_rejected(&validator, &bad_digest, "malformed sha256 digest");

    let mut unknown_severity = base.clone();
    unknown_severity["diagnostics"] = json!([{
        "code": "GB_SEQUENCE_NORMALIZED",
        "severity": "fatal",
        "message": "severity outside the enum",
    }]);
    assert_rejected(&validator, &unknown_severity, "unknown diagnostic severity");

    let mut unknown_property = base.clone();
    unknown_property["unexpected_field"] = json!(true);
    assert_rejected(&validator, &unknown_property, "unknown top-level property");

    let mut bad_position = base.clone();
    bad_position["diagnostics"] = json!([{
        "code": "GB_LOCATION_MALFORMED",
        "severity": "error",
        "message": "genbank position without a line",
        "position": {"kind": "genbank", "offset": 10},
    }]);
    assert_rejected(
        &validator,
        &bad_position,
        "GenBank position without the required line",
    );
}

#[test]
fn preserved_data_base64_is_an_annotation_only() {
    // JSON Schema 2020-12 treats contentEncoding as an annotation. The schema
    // intentionally cannot decode preserved.data: producing valid base64 is the
    // adapter's job and is checked by adapter/validate() tests. Pinning the
    // annotation behavior keeps a future format-assertion switch from silently
    // changing the contract.
    let validator = compiled_schema(REPORT_SCHEMA_FILE);
    let mut document = example_for(REPORT_SCHEMA_ID);
    document["preserved"] = json!([{
        "what": "unknown_packet",
        "encoding": "base64",
        "data": "this is not base64!!!",
    }]);
    assert!(
        validator.is_valid(&document),
        "contentEncoding must remain annotation-only in schema validation"
    );
}

#[test]
fn committed_goldens_validate_against_the_record_schema() {
    let validator = compiled_schema(RECORD_SCHEMA_FILE);
    let directory = workspace_root().join("testdata/expected");
    let entries = fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("read goldens dir {}: {error}", directory.display()));
    let mut count = 0_usize;
    for entry in entries {
        let path = entry.expect("readable golden entry").path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let document = read_json(&path);
        let errors = validation_errors(&validator, &document);
        assert!(
            errors.is_empty(),
            "{} failed schema validation:\n{}",
            path.display(),
            errors.join("\n")
        );
        count += 1;
    }
    assert!(
        count > 0,
        "testdata/expected must contain committed goldens"
    );
}
