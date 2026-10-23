//! Canonical JSON round-trip and diagnostic ordering for the import report.
//!
//! The byte rules are fixed by docs/design/canonical-json-profile.md and the
//! diagnostic order by docs/design/diagnostics.md.

use std::fs;
use std::path::PathBuf;

use sequon_io::json::{parse_import_report, to_canonical_report_json_bytes};
use sequon_io::report::{
    Diagnostic, Dropped, ImportReport, Preserved, PreservedEncoding, Severity, SourcePosition,
};

fn report_example() -> Vec<u8> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    fs::read(root.join("../../schemas/examples/report.example.json"))
        .expect("report example is committed")
}

#[test]
fn report_example_round_trips_byte_identically() {
    let bytes = report_example();
    let report = parse_import_report(&bytes).expect("example parses");
    let reserialized = to_canonical_report_json_bytes(&report).expect("example serializes");
    assert_eq!(
        String::from_utf8_lossy(&reserialized),
        String::from_utf8_lossy(&bytes),
        "the accepted example must round-trip through the report types unchanged"
    );
}

#[test]
fn nullable_diagnostic_fields_are_emitted_as_null() {
    let diagnostic = Diagnostic::new("GB_EMPTY_INPUT", Severity::Error, "empty input");
    let json = serde_json::to_string(&diagnostic).expect("diagnostic serializes");
    assert!(
        json.contains("\"position\":null"),
        "position must be emitted: {json}"
    );
    assert!(
        json.contains("\"record_index\":null"),
        "record_index must be emitted: {json}"
    );
    assert_eq!(
        json,
        "{\"code\":\"GB_EMPTY_INPUT\",\"severity\":\"error\",\"message\":\"empty input\",\"position\":null,\"record_index\":null}"
    );
}

#[test]
fn optional_dropped_and_preserved_fields_are_omitted_when_unknown() {
    let dropped = Dropped {
        what: "feature".to_string(),
        reason: "unsupported_location".to_string(),
        record_index: Some(0),
        position: None,
        packet_type: None,
        offset: None,
        length: None,
        digest: None,
        feature_key: Some("CDS".to_string()),
        location_text: Some("one-of(1..2)".to_string()),
    };
    let json = serde_json::to_string(&dropped).expect("dropped serializes");
    assert!(
        !json.contains("position"),
        "absent position must be omitted: {json}"
    );
    assert!(
        json.contains("feature_key"),
        "present field must be emitted: {json}"
    );

    let preserved = Preserved {
        what: "unknown_packet".to_string(),
        encoding: PreservedEncoding::Base64,
        data: "AAEC".to_string(),
        packet_type: None,
        offset: None,
        length: Some(3),
        digest: None,
        record_index: None,
    };
    let json = serde_json::to_string(&preserved).expect("preserved serializes");
    assert!(json.contains("\"encoding\":\"base64\""), "{json}");
    assert!(!json.contains("packet_type"), "{json}");
}

#[test]
fn diagnostics_sort_positioned_events_first_then_by_position_and_code() {
    let mut report = ImportReport::new(
        sequon_model::SourceFormat::Genbank,
        sequon_model::DigestValue::parse(&format!("sha256:{}", "0".repeat(64))).unwrap(),
        10,
        sequon_model::Producer {
            kind: sequon_model::ProducerKind::Parser,
            name: "sequon-io".to_string(),
            version: "0.1.0".to_string(),
        },
        None,
    );
    report.diagnostics = vec![
        Diagnostic::new("GB_UNKNOWN_FEATURE_KEY", Severity::Info, "d").with_position(
            SourcePosition::Genbank {
                line: 20,
                column: None,
            },
        ),
        Diagnostic::new("GB_SEQUENCE_NORMALIZED", Severity::Info, "b"),
        Diagnostic::new("GB_LOCATION_MALFORMED", Severity::Error, "c").with_position(
            SourcePosition::Genbank {
                line: 20,
                column: Some(5),
            },
        ),
        Diagnostic::new("GB_CRLF_NORMALIZED", Severity::Info, "a"),
    ];
    report.sort_diagnostics();
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(
        codes,
        vec![
            "GB_UNKNOWN_FEATURE_KEY",
            "GB_LOCATION_MALFORMED",
            "GB_CRLF_NORMALIZED",
            "GB_SEQUENCE_NORMALIZED",
        ],
        "positioned events first by line then column (absent column sorts as 0), then code; equal keys keep emission order"
    );
}
