//! Canonical JSON tests for sequon-io.
//!
//! These tests pin the byte-level contract in
//! docs/design/canonical-json-profile.md and the field order fixed by
//! docs/design/canonical-model.md. The accepted schema example is used as the
//! reference input, and the record schema file is read to derive the expected
//! top-level field order instead of duplicating it by hand.

use std::collections::BTreeMap;

use sequon_io::json::{parse_canonical_json, to_canonical_json_bytes};
use sequon_model::{JsonValue, Record};

const EXAMPLE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../schemas/examples/record.example.json"
));

const RECORD_SCHEMA: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../schemas/sequon-record-1.schema.json"
));

fn example_record() -> Record {
    parse_canonical_json(EXAMPLE).expect("the accepted schema example must parse")
}

fn canonical_text(record: &Record) -> String {
    let bytes = to_canonical_json_bytes(record).expect("canonical serialization must succeed");
    String::from_utf8(bytes).expect("canonical JSON is UTF-8")
}

/// Returns the member keys of a pretty-printed object at exactly one indentation
/// level, in document order. Objects and arrays nest one level deeper, so this
/// reconstructs the canonical field order without needing a JSON parser that
/// preserves key order.
fn keys_at_indent(text: &str, indent: usize) -> Vec<String> {
    let prefix = " ".repeat(indent);
    let mut keys = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix(prefix.as_str()) else {
            continue;
        };
        let Some(rest) = rest.strip_prefix('"') else {
            continue;
        };
        if let Some((key, tail)) = rest.split_once('"') {
            if tail.starts_with(':') {
                keys.push(key.to_owned());
            }
        }
    }
    keys
}

/// Reads the declaration order of the top-level record properties from the
/// pretty-printed record schema. Top-level properties are the keys indented by
/// four spaces directly inside the top-level properties object.
fn schema_top_level_property_order(schema: &str) -> Vec<String> {
    let mut order = Vec::new();
    let mut inside_properties = false;
    for line in schema.lines() {
        if !inside_properties {
            if line == "  \"properties\": {" {
                inside_properties = true;
            }
            continue;
        }
        if line == "  }," || line == "  }" {
            break;
        }
        if let Some(rest) = line.strip_prefix("    \"") {
            if let Some((key, tail)) = rest.split_once("\":") {
                if tail.is_empty() || tail.starts_with(' ') {
                    order.push(key.to_owned());
                }
            }
        }
    }
    order
}

#[test]
fn schema_example_round_trips_byte_for_byte() {
    let record = example_record();
    record
        .validate()
        .expect("the accepted example must validate");
    sequon_algo::verify_digests(&record).expect("the accepted example digests must verify");

    let bytes = to_canonical_json_bytes(&record).expect("serialization must succeed");
    assert_eq!(
        bytes, EXAMPLE,
        "serializing the accepted example must reproduce the committed bytes"
    );
}

#[test]
fn top_level_field_order_equals_schema_declaration_order() {
    let expected = schema_top_level_property_order(RECORD_SCHEMA);
    assert_eq!(
        expected,
        vec![
            "schema",
            "kind",
            "molecule_type",
            "topology",
            "name",
            "description",
            "length",
            "sequence",
            "digests",
            "features",
            "provenance",
            "extras",
        ],
        "the record schema declaration order must match the accepted field order"
    );

    let text = canonical_text(&example_record());
    assert_eq!(keys_at_indent(&text, 2), expected);
}

#[test]
fn nested_field_order_matches_the_accepted_document() {
    let text = canonical_text(&example_record());

    assert_eq!(
        keys_at_indent(&text, 4),
        vec![
            "algorithm",
            "sequence",
            "canonical",
            "producer",
            "source_format",
            "source_digest",
            "source_bytes",
            "export_version",
            "genbank",
        ],
        "digests, provenance and extras namespace order"
    );

    let feature_members = ["key", "so_term", "name", "location", "qualifiers"];
    let mut expected = Vec::new();
    for _ in 0..3 {
        expected.extend(feature_members.iter().copied());
    }
    expected.extend(["kind", "name", "version"]);
    expected.push("header");
    assert_eq!(
        keys_at_indent(&text, 6),
        expected,
        "feature and producer order"
    );

    assert_eq!(
        keys_at_indent(&text, 8),
        vec![
            "operator", "strand", "segments", "operator", "strand", "segments", "operator",
            "strand", "segments",
        ],
        "location order"
    );

    assert_eq!(
        keys_at_indent(&text, 12),
        vec![
            "start",
            "end",
            "start_partial",
            "end_partial",
            "start",
            "end",
            "start_partial",
            "end_partial",
            "start",
            "end",
            "start_partial",
            "end_partial",
            "start",
            "end",
            "start_partial",
            "end_partial",
        ],
        "segment order"
    );
}

#[test]
fn output_uses_lf_two_spaces_and_exactly_one_trailing_newline() {
    let bytes = to_canonical_json_bytes(&example_record()).expect("serialization must succeed");
    assert!(!bytes.contains(&b'\r'), "no CR bytes");
    assert!(!bytes.contains(&b'\t'), "no tab bytes");
    assert!(bytes.ends_with(b"\n"), "one trailing newline");
    assert!(!bytes.ends_with(b"\n\n"), "exactly one trailing newline");

    let text = String::from_utf8(bytes).expect("UTF-8");
    for line in text.lines() {
        assert_eq!(line, line.trim_end(), "no trailing whitespace");
    }
    assert!(text.contains("\n  \"kind\": \"dna\",\n"));
    assert!(text.contains("\n  \"length\": 2686,\n"));
}

#[test]
fn nullable_fields_are_emitted_as_null() {
    let mut record = example_record();
    record.molecule_type = None;
    record.description = None;
    record.features[0].so_term = None;
    record.features[0].name = None;
    record.provenance.export_version = None;

    let text = canonical_text(&record);
    assert!(text.contains("\"molecule_type\": null"));
    assert!(text.contains("\"description\": null"));
    assert!(text.contains("\"so_term\": null"));
    assert!(text.contains("\"name\": null"));
    assert!(text.contains("\"export_version\": null"));

    let reparsed = parse_canonical_json(text.as_bytes()).expect("nulls round-trip");
    assert_eq!(reparsed, record);
}

#[test]
fn absent_nullable_fields_are_accepted() {
    let original = std::str::from_utf8(EXAMPLE).expect("example is UTF-8");
    let without_nullable = original
        .replace("  \"molecule_type\": \"ds-DNA\",\n", "")
        .replace(
            "  \"description\": \"Synthetic 2686 bp example plasmid, not a real sequence.\",\n",
            "",
        )
        .replace("    \"export_version\": null,\n", "");
    let record = parse_canonical_json(without_nullable.as_bytes())
        .expect("absent nullable fields must be accepted");
    assert_eq!(record.molecule_type, None);
    assert_eq!(record.description, None);
    assert_eq!(record.provenance.export_version, None);

    // The canonical serializer restores them as explicit nulls.
    let text = canonical_text(&record);
    assert!(text.contains("\"molecule_type\": null"));
    assert!(text.contains("\"export_version\": null"));
}

#[test]
fn extras_keys_are_sorted_and_round_trip() {
    // Deserialize a document whose extras namespaces and nested keys are in
    // reverse order; the canonical form must re-emit them sorted.
    let original = std::str::from_utf8(EXAMPLE).expect("example is UTF-8");
    let unsorted = original.replace(
        "  \"extras\": {\n",
        "  \"extras\": {\n    \"zulu_note\": { \"zeta\": 1, \"alpha\": 2 },\n",
    );
    let record = parse_canonical_json(unsorted.as_bytes()).expect("out-of-order keys parse");
    let text = canonical_text(&record);
    let genbank = text
        .find("\"genbank\": {")
        .expect("genbank namespace present");
    let zulu = text
        .find("\"zulu_note\": {")
        .expect("zulu_note namespace preserved");
    assert!(
        genbank < zulu,
        "extras namespace keys must be re-emitted in sorted order"
    );
    let alpha = text.find("\"alpha\": 2,").expect("nested alpha present");
    let zeta = text.find("\"zeta\": 1").expect("nested zeta present");
    assert!(alpha < zeta, "nested extras object keys must be sorted");

    let mut record = example_record();
    let mut nested = BTreeMap::new();
    nested.insert("zeta".to_owned(), JsonValue::from(1_i64));
    nested.insert("alpha".to_owned(), JsonValue::from(2_i64));
    record.extras.insert("probe", JsonValue::Object(nested));
}

#[test]
fn deterministic_repeated_serialization() {
    let record = example_record();
    let first = to_canonical_json_bytes(&record).expect("first");
    let second = to_canonical_json_bytes(&record).expect("second");
    assert_eq!(first, second);

    let reparsed = parse_canonical_json(&first).expect("reparse");
    assert_eq!(reparsed, record);
    let reserialized = to_canonical_json_bytes(&reparsed).expect("reserialize");
    assert_eq!(first, reserialized);
}

#[test]
fn unknown_fields_are_rejected() {
    let original = std::str::from_utf8(EXAMPLE).expect("example is UTF-8");

    let top_level = original.replace("{\n", "{\n  \"unexpected_top_level\": true,\n");
    let error = parse_canonical_json(top_level.as_bytes()).expect_err("unknown top-level field");
    assert!(
        error.to_string().contains("unknown field"),
        "unexpected error: {error}"
    );

    let feature = original.replace(
        "      \"key\": \"source\",\n",
        "      \"key\": \"source\",\n      \"unexpected_feature\": 1,\n",
    );
    let error = parse_canonical_json(feature.as_bytes()).expect_err("unknown feature field");
    assert!(
        error.to_string().contains("unknown field"),
        "unexpected error: {error}"
    );

    let segment = original.replace(
        "            \"start\": 0,\n",
        "            \"start\": 0,\n            \"unexpected_segment\": 1,\n",
    );
    let error = parse_canonical_json(segment.as_bytes()).expect_err("unknown segment field");
    assert!(
        error.to_string().contains("unknown field"),
        "unexpected error: {error}"
    );
}

#[test]
fn floating_point_numbers_are_rejected_everywhere() {
    let original = std::str::from_utf8(EXAMPLE).expect("example is UTF-8");

    let extras_float = original.replace(
        "  \"extras\": {\n",
        "  \"extras\": {\n    \"probe\": 1.5,\n",
    );
    let error = parse_canonical_json(extras_float.as_bytes())
        .expect_err("floating point in extras must be rejected");
    assert!(
        error.to_string().contains("floating-point number"),
        "unexpected error: {error}"
    );

    let model_float = original.replace("\"length\": 2686,", "\"length\": 2686.5,");
    let error = parse_canonical_json(model_float.as_bytes())
        .expect_err("floating point in a model field must be rejected");
    assert!(
        error.to_string().contains("floating point"),
        "unexpected error: {error}"
    );
}

#[test]
fn duplicate_extras_keys_are_rejected() {
    let original = std::str::from_utf8(EXAMPLE).expect("example is UTF-8");
    let duplicate = original.replace(
        "  \"extras\": {\n",
        "  \"extras\": {\n    \"genbank\": {},\n",
    );
    let error =
        parse_canonical_json(duplicate.as_bytes()).expect_err("duplicate object key must fail");
    assert!(
        error.to_string().contains("duplicate object key"),
        "unexpected error: {error}"
    );
}

#[test]
fn enum_values_are_checked_by_serde() {
    let original = std::str::from_utf8(EXAMPLE).expect("example is UTF-8");
    let bad_kind = original.replace("\"kind\": \"dna\"", "\"kind\": \"plasmid\"");
    let error = parse_canonical_json(bad_kind.as_bytes()).expect_err("unknown kind");
    assert!(
        error.to_string().contains("unknown variant"),
        "unexpected error: {error}"
    );
}
