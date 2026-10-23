//! Embedded-schema checks: the bytes a platform pins are the committed bytes.
//!
//! `sequon-model` and `sequon-io` publish their JSON Schema documents as
//! `&'static str` constants (`include_str!` through a relative symlink under the
//! crate directory). A consumer hashes those bytes to pin the contract, so the
//! library bytes must stay identical to the authoritative files at the
//! repository root.
//!
//! This test guards the three properties that pinning relies on:
//!
//! - the embedded bytes are byte-identical to the root `schemas/` files, so a
//!   computed SHA-256 describes the committed document;
//! - the embedded documents parse as JSON objects;
//! - each embedded `$id` equals the exported `*_SCHEMA_URN` constant, so the
//!   identity a consumer pins without parsing JSON is the identity in the bytes.
//!
//! Digests are compared against each other and never hardcoded, so a deliberate
//! schema change does not require touching this test. The conformance crate is
//! `publish = false`, so it may read the root files directly with
//! `include_str!("../../../schemas/...")`; the library crates cannot.

use serde_json::{Map, Value};

use sequon_io::{REPORT_SCHEMA, REPORT_SCHEMA_URN};
use sequon_model::{RECORD_SCHEMA, RECORD_SCHEMA_URN};

/// Authoritative record schema, read straight from the repository root.
const ROOT_RECORD_SCHEMA: &str = include_str!("../../../schemas/sequon-record-1.schema.json");

/// Authoritative import-report schema, read straight from the repository root.
const ROOT_REPORT_SCHEMA: &str = include_str!("../../../schemas/import-report-1.schema.json");

/// Parses a schema document that must be a JSON object.
fn parse_object(document: &str, constant: &str) -> Map<String, Value> {
    match serde_json::from_str(document) {
        Ok(Value::Object(object)) => object,
        Ok(other) => panic!("{constant} must be a JSON object, found {other}"),
        Err(error) => panic!("{constant} is not valid JSON: {error}"),
    }
}

#[test]
fn embedded_schemas_are_byte_identical_to_the_root_files() {
    assert_eq!(
        RECORD_SCHEMA, ROOT_RECORD_SCHEMA,
        "sequon_model::RECORD_SCHEMA must embed schemas/sequon-record-1.schema.json unmodified"
    );
    assert_eq!(
        REPORT_SCHEMA, ROOT_REPORT_SCHEMA,
        "sequon_io::REPORT_SCHEMA must embed schemas/import-report-1.schema.json unmodified"
    );

    // A &str comparison also compares bytes here, but assert the byte view
    // explicitly: a consumer hashes bytes, not text.
    assert_eq!(RECORD_SCHEMA.as_bytes(), ROOT_RECORD_SCHEMA.as_bytes());
    assert_eq!(REPORT_SCHEMA.as_bytes(), ROOT_REPORT_SCHEMA.as_bytes());
}

#[test]
fn embedded_schemas_parse_as_objects() {
    parse_object(RECORD_SCHEMA, "sequon_model::RECORD_SCHEMA");
    parse_object(REPORT_SCHEMA, "sequon_io::REPORT_SCHEMA");
}

#[test]
fn embedded_schemas_declare_the_exported_urn_identity() {
    let record = parse_object(RECORD_SCHEMA, "sequon_model::RECORD_SCHEMA");
    assert_eq!(
        record.get("$id").and_then(Value::as_str),
        Some(RECORD_SCHEMA_URN),
        "the $id inside RECORD_SCHEMA must equal sequon_model::RECORD_SCHEMA_URN"
    );

    let report = parse_object(REPORT_SCHEMA, "sequon_io::REPORT_SCHEMA");
    assert_eq!(
        report.get("$id").and_then(Value::as_str),
        Some(REPORT_SCHEMA_URN),
        "the $id inside REPORT_SCHEMA must equal sequon_io::REPORT_SCHEMA_URN"
    );
}

#[test]
fn embedded_schema_digests_match_the_root_file_digests() {
    assert_eq!(
        sequon_algo::bytes_digest(RECORD_SCHEMA.as_bytes()),
        sequon_algo::bytes_digest(ROOT_RECORD_SCHEMA.as_bytes()),
        "the embedded record schema bytes must hash to the committed file digest"
    );
    assert_eq!(
        sequon_algo::bytes_digest(REPORT_SCHEMA.as_bytes()),
        sequon_algo::bytes_digest(ROOT_REPORT_SCHEMA.as_bytes()),
        "the embedded report schema bytes must hash to the committed file digest"
    );
}
