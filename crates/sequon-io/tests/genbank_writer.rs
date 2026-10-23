#![cfg(feature = "genbank")]

//! GenBank writer tests: layout, diagnostics, error cases and the M3
//! round-trip matrix (fixture -> record -> GenBank -> canonical record).

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use sequon_io::api::{ImportOptions, WriteError, WriteOptions, WriteOutcome};
use sequon_io::json::{parse_canonical_json, to_canonical_json_bytes};
use sequon_io::report::Severity;
use sequon_io::{import_genbank, write_genbank};
use sequon_model::{
    Extras, Feature, JsonValue, Kind, Location, MoleculeType, Record, Segment, OPERATOR_SPAN,
};

const ROUND_TRIP_FIXTURES: &[&str] = &[
    "001-minimal-cds",
    "010-circular-plasmid",
    "020-join-complement",
    "030-order",
    "040-near-origin",
    "050-partial",
    "060-duplicate-qualifiers",
    "070-unknown-key",
    "080-multi-record",
    "090-locus-length-mismatch",
    "100-crlf",
    "110-no-features",
    "150-large-1mb",
    "163-name-hint",
];

const GOLDEN_STEMS: &[&str] = &[
    "001-minimal-cds",
    "010-circular-plasmid",
    "020-join-complement",
    "030-order",
    "040-near-origin",
    "050-partial",
    "060-duplicate-qualifiers",
    "070-unknown-key",
    "080-multi-record.0",
    "080-multi-record.1",
    "090-locus-length-mismatch",
    "100-crlf",
    "110-no-features",
    "150-large-1mb",
    "163-name-hint",
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata")
}

fn fixture(name: &str) -> Vec<u8> {
    fs::read(root().join("genbank").join(format!("{name}.gb"))).expect("fixture file")
}

fn golden(name: &str) -> Record {
    let bytes =
        fs::read(root().join("expected").join(format!("{name}.json"))).expect("golden file");
    parse_canonical_json(&bytes).expect("golden is canonical JSON")
}

fn options_for(name: &str) -> ImportOptions {
    let mut options = ImportOptions::default();
    if name == "163-name-hint" {
        options.name_hint = Some("HINT163".to_string());
    }
    options
}

fn import_records(name: &str) -> Vec<Record> {
    import_genbank(&fixture(name), &options_for(name))
        .expect("fixture imports")
        .records
}

fn one(name: &str) -> Record {
    let mut records = import_records(name);
    assert_eq!(records.len(), 1, "{name}");
    records.remove(0)
}

fn write(record: &Record) -> WriteOutcome {
    write_genbank(record, &WriteOptions::default()).expect("record writes")
}

fn write_with(record: &Record, options: &WriteOptions) -> WriteOutcome {
    write_genbank(record, options).expect("record writes")
}

fn text(outcome: &WriteOutcome) -> String {
    String::from_utf8(outcome.bytes.clone()).expect("writer output is UTF-8")
}

fn assert_equal_ignoring_provenance(left: &Record, right: &Record) {
    let mut comparable = right.clone();
    comparable.provenance = left.provenance.clone();
    assert_eq!(left, &comparable, "records differ beyond provenance");
    right.validate().expect("re-imported record validates");
    sequon_algo::verify_digests(right).expect("re-imported digests verify");
}

fn assert_round_trip(record: &Record) {
    let outcome = write(record);
    let imported =
        import_genbank(&outcome.bytes, &ImportOptions::default()).expect("writer output imports");
    assert_eq!(imported.records.len(), 1, "one record re-imported");
    assert_equal_ignoring_provenance(record, &imported.records[0]);
}

fn codes(outcome: &WriteOutcome) -> Vec<&str> {
    outcome
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect()
}

fn has_line(text: &str, needle: &str) -> bool {
    text.lines().any(|line| line.contains(needle))
}

fn with_header(record: &Record, entries: &[(&str, &str)], division: Option<&str>) -> Record {
    let mut record = record.clone();
    let mut payload = BTreeMap::new();
    let mut header = Vec::new();
    for (key, value) in entries {
        let mut entry = BTreeMap::new();
        entry.insert("key".to_string(), JsonValue::from(*key));
        entry.insert("value".to_string(), JsonValue::from(*value));
        header.push(JsonValue::Object(entry));
    }
    payload.insert("header".to_string(), JsonValue::Array(header));
    if let Some(division) = division {
        payload.insert("division".to_string(), JsonValue::from(division));
    }
    let mut extras = Extras::new();
    extras.insert("genbank", JsonValue::Object(payload));
    record.extras = extras;
    record
}

fn record_with_location(location: Location) -> Record {
    let mut record = one("020-join-complement");
    record.features = vec![Feature {
        key: "misc_feature".to_string(),
        so_term: None,
        name: None,
        location,
        qualifiers: Vec::new(),
    }];
    record
}

#[test]
fn double_write_is_byte_identical_for_every_writer_accepted_fixture() {
    for name in ROUND_TRIP_FIXTURES {
        for record in import_records(name) {
            let first = write(&record);
            let second = write(&record);
            assert_eq!(first.bytes, second.bytes, "{name} bytes");
            assert_eq!(first.diagnostics, second.diagnostics, "{name} diagnostics");
        }
    }
}

#[test]
fn import_write_import_round_trip_for_every_writer_accepted_fixture() {
    for name in ROUND_TRIP_FIXTURES {
        for record in import_records(name) {
            assert_round_trip(&record);
        }
    }
}

#[test]
fn goldens_round_trip_through_writer() {
    for stem in GOLDEN_STEMS {
        let record = golden(stem);
        assert_round_trip(&record);
    }

    // molecule_type = other is a successful import but not representable.
    let other = golden("162-unknown-molecule");
    let error = write_genbank(&other, &WriteOptions::default()).expect_err("other must fail");
    assert!(matches!(error, WriteError::UnsupportedFeature { .. }));
    assert!(error
        .to_string()
        .contains("WRITE_UNSUPPORTED_MOLECULE_TYPE"));
}

#[test]
fn unsupported_kind_is_rejected() {
    let mut record = one("001-minimal-cds");
    record.kind = Kind::Rna;
    let error = write_genbank(&record, &WriteOptions::default()).expect_err("RNA has no writer");
    assert!(matches!(
        error,
        WriteError::UnsupportedKind { kind: Kind::Rna }
    ));
}

#[test]
fn locus_and_origin_layout() {
    let outcome = write(&one("001-minimal-cds"));
    let output = text(&outcome);
    let lines: Vec<&str> = output.lines().collect();
    let locus = lines[0];
    assert_eq!(locus.len(), 79);
    assert_eq!(&locus[..5], "LOCUS");
    assert_eq!(locus[12..28].trim(), "SYN001");
    assert_eq!(locus[29..40].trim(), "60");
    assert_eq!(&locus[41..43], "bp");
    assert_eq!(locus[47..53].trim(), "DNA");
    assert_eq!(locus[55..63].trim(), "linear");
    assert_eq!(&locus[64..67], "SYN");
    assert!(locus.ends_with("01-JAN-2000"));
    assert!(output.ends_with("//\n"));
    assert!(!output.contains('\r'));

    let origin_index = lines
        .iter()
        .position(|line| *line == "ORIGIN")
        .expect("ORIGIN");
    let first_bases = lines[origin_index + 1];
    assert_eq!(first_bases.len(), 75);
    assert_eq!(first_bases[..9].trim(), "1");
    let bases: String = first_bases[10..].replace(' ', "");
    assert_eq!(bases, one("001-minimal-cds").sequence);
    assert_eq!(lines.last(), Some(&"//"));
}

#[test]
fn blank_division_keeps_the_date_column() {
    let record = one("110-no-features");
    let output = text(&write(&record));
    let locus = output.lines().next().expect("LOCUS");
    assert_eq!(locus.len(), 79);
    assert_eq!(&locus[64..67], "   ");
    assert!(locus[68..].starts_with("01-JAN-2000"));
}

#[test]
fn ss_dna_molecule_token_is_preserved() {
    let mut record = one("001-minimal-cds");
    record.molecule_type = Some(MoleculeType::SsDna);
    let output = text(&write(&record));
    assert!(output.lines().next().expect("LOCUS").contains("ss-DNA"));
    let imported = import_genbank(output.as_bytes(), &ImportOptions::default())
        .expect("writer output imports");
    assert_eq!(imported.records[0].molecule_type, Some(MoleculeType::SsDna));
}

#[test]
fn header_restoration_from_extras() {
    let record = one("001-minimal-cds");
    let output = write(&record);
    let output = text(&output);
    assert!(has_line(&output, "DEFINITION  minimal synthetic record."));
    assert!(has_line(&output, "ACCESSION   SYN001"));
    assert!(has_line(&output, "VERSION     SYN001.1"));

    let reimported = import_genbank(output.as_bytes(), &ImportOptions::default())
        .expect("writer output imports")
        .records;
    assert_eq!(reimported[0].extras, record.extras);
    assert_eq!(reimported[0].description, record.description);

    let synthesized = with_header(&record, &[], None);
    let mut synthesized = synthesized;
    synthesized.description = Some("description only.".to_string());
    let output = text(&write(&synthesized));
    assert!(has_line(&output, "DEFINITION  description only."));
    assert!(!has_line(&output, "ACCESSION"));
}

#[test]
fn header_restoration_preserves_organism_reference_and_comment() {
    let mut record = one("001-minimal-cds");
    record.description = Some("header block.".to_string());
    record = with_header(
        &record,
        &[
            ("DEFINITION", "header block."),
            ("SOURCE", "synthetic construct"),
            (
                "ORGANISM",
                "synthetic construct\n            synthetic; construct.",
            ),
            (
                "REFERENCE",
                "1  (bases 1 to 60)\n  AUTHORS   Doe,J.\n  TITLE     A title",
            ),
            (
                "COMMENT",
                "first comment line\n            second comment line",
            ),
        ],
        Some("SYN"),
    );
    let output = text(&write(&record));
    assert!(has_line(&output, "DEFINITION  header block."));
    assert!(has_line(&output, "SOURCE      synthetic construct"));
    assert!(has_line(&output, "  ORGANISM  synthetic construct"));
    assert!(has_line(&output, "            synthetic; construct."));
    assert!(has_line(&output, "REFERENCE   1  (bases 1 to 60)"));
    assert!(has_line(&output, "  AUTHORS   Doe,J."));
    assert!(has_line(&output, "COMMENT     first comment line"));
    assert!(has_line(&output, "            second comment line"));

    let imported = import_genbank(output.as_bytes(), &ImportOptions::default())
        .expect("writer output imports");
    assert_equal_ignoring_provenance(&record, &imported.records[0]);
    assert_eq!(imported.records[0].extras, record.extras);
}

#[test]
fn header_definition_wins_over_description_and_structural_keys_are_ignored() {
    let mut record = one("001-minimal-cds");
    record.description = Some("description loses.".to_string());
    record = with_header(
        &record,
        &[
            ("LOCUS", "ignored"),
            ("DEFINITION", "header wins."),
            ("FEATURES", "ignored"),
            ("ORIGIN", "ignored"),
        ],
        Some("SYN"),
    );
    let output = text(&write(&record));
    assert!(has_line(&output, "DEFINITION  header wins."));
    assert!(!has_line(&output, "description loses."));
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.starts_with("LOCUS "))
            .count(),
        1
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.starts_with("FEATURES "))
            .count(),
        1
    );
    assert_eq!(lines.iter().filter(|line| **line == "ORIGIN").count(), 1);
    let imported = import_genbank(output.as_bytes(), &ImportOptions::default())
        .expect("writer output imports");
    assert_eq!(
        imported.records[0].description.as_deref(),
        Some("header wins.")
    );
}

#[test]
fn include_header_extras_false_emits_only_synthesized_header() {
    let mut record = one("001-minimal-cds");
    record.description = Some("synthesized definition.".to_string());
    let options = WriteOptions {
        include_header_extras: false,
        ..WriteOptions::default()
    };
    let output = text(&write_with(&record, &options));
    assert!(has_line(&output, "DEFINITION  synthesized definition."));
    assert!(!has_line(&output, "ACCESSION"));
    assert!(!has_line(&output, "VERSION"));
    assert!(has_line(
        &output,
        "FEATURES             Location/Qualifiers"
    ));
    let imported = import_genbank(output.as_bytes(), &ImportOptions::default())
        .expect("writer output imports");
    assert_eq!(
        imported.records[0].description.as_deref(),
        Some("synthesized definition.")
    );
}

#[test]
fn name_truncation_warns_and_truncates() {
    let mut record = one("001-minimal-cds");
    record.name = "THIS_NAME_IS_LONGER_THAN_SIXTEEN".to_string();
    let outcome = write(&record);
    assert_eq!(codes(&outcome), vec!["WRITE_NAME_TRUNCATED"]);
    assert_eq!(outcome.diagnostics[0].severity, Severity::Warning);
    let output = text(&outcome);
    assert!(output
        .lines()
        .next()
        .expect("LOCUS")
        .contains("THIS_NAME_IS_LON"));
    let imported = import_genbank(output.as_bytes(), &ImportOptions::default())
        .expect("writer output imports");
    assert_eq!(imported.records[0].name, "THIS_NAME_IS_LON");
}

#[test]
fn embedded_quote_is_replaced_with_two_apostrophes() {
    let mut record = one("001-minimal-cds");
    record.features[0].qualifiers[0].value = Some("a\"b".to_string());
    let outcome = write(&record);
    assert_eq!(codes(&outcome), vec!["WRITE_QUOTE_NORMALIZED"]);
    assert_eq!(outcome.diagnostics[0].severity, Severity::Info);
    let output = text(&outcome);
    assert!(has_line(&output, "a''b"));
    let imported = import_genbank(output.as_bytes(), &ImportOptions::default())
        .expect("writer output imports");
    assert_eq!(
        imported.records[0].features[0].qualifiers[0]
            .value
            .as_deref(),
        Some("a''b")
    );
}

#[test]
fn strand_zero_is_written_as_forward_and_warned() {
    let mut record = one("001-minimal-cds");
    record.features[0].location.strand = 0;
    let outcome = write(&record);
    assert_eq!(codes(&outcome), vec!["WRITE_STRAND_LOST"]);
    assert_eq!(outcome.diagnostics[0].severity, Severity::Warning);
    let output = text(&outcome);
    assert!(!has_line(&output, "complement("));
    let imported = import_genbank(output.as_bytes(), &ImportOptions::default())
        .expect("writer output imports");
    assert_eq!(imported.records[0].features[0].location.strand, 1);
}

#[test]
fn missing_molecule_type_is_derived_with_info() {
    let mut record = one("001-minimal-cds");
    record.molecule_type = None;
    let outcome = write(&record);
    assert_eq!(codes(&outcome), vec!["WRITE_MOLECULE_TYPE_DERIVED"]);
    assert_eq!(outcome.diagnostics[0].severity, Severity::Info);
    let output = text(&outcome);
    assert!(output.lines().next().expect("LOCUS").contains("DNA"));
    let imported = import_genbank(output.as_bytes(), &ImportOptions::default())
        .expect("writer output imports");
    assert_eq!(imported.records[0].molecule_type, Some(MoleculeType::DsDna));
}

#[test]
fn date_comes_from_options_or_fixed_placeholder() {
    let record = one("001-minimal-cds");
    let default = text(&write(&record));
    assert!(default
        .lines()
        .next()
        .expect("LOCUS")
        .ends_with("01-JAN-2000"));

    let options = WriteOptions {
        date: Some("31-DEC-2025".to_string()),
        ..WriteOptions::default()
    };
    let dated = text(&write_with(&record, &options));
    assert!(dated
        .lines()
        .next()
        .expect("LOCUS")
        .ends_with("31-DEC-2025"));

    for invalid in ["2025-12-31", "31-dec-2025", "32-JAN-2025", "1-JAN-2025"] {
        let options = WriteOptions {
            date: Some(invalid.to_string()),
            ..WriteOptions::default()
        };
        let error = write_genbank(&record, &options).expect_err("invalid date");
        assert!(
            matches!(error, WriteError::InvalidRecord { .. }),
            "{invalid}"
        );
    }
}

#[test]
fn line_width_wraps_qualifier_values_and_reimports_equal() {
    let record = one("060-duplicate-qualifiers");
    let options = WriteOptions {
        line_width: 45,
        ..WriteOptions::default()
    };
    let outcome = write_with(&record, &options);
    let output = text(&outcome);
    let note_line = output
        .lines()
        .find(|line| line.contains("/note="))
        .expect("note qualifier");
    assert!(
        note_line.len() <= 45 || !note_line.contains(' '),
        "{note_line}"
    );
    assert!(output.lines().any(|line| line.starts_with(&" ".repeat(21))));
    let imported = import_genbank(output.as_bytes(), &ImportOptions::default())
        .expect("writer output imports");
    assert_equal_ignoring_provenance(&record, &imported.records[0]);
}

#[test]
fn location_serialization_forms() {
    let span = record_with_location(Location::span(1, 9, 30));
    let output = text(&write(&span));
    assert!(has_line(&output, "misc_feature    10..30"));

    let single = record_with_location(Location::span(1, 9, 10));
    assert!(has_line(&text(&write(&single)), "misc_feature    10"));

    let partial = record_with_location(Location {
        operator: OPERATOR_SPAN.to_string(),
        strand: 1,
        segments: vec![Segment::with_boundaries(9, 30, true, true)],
    });
    assert!(has_line(
        &text(&write(&partial)),
        "misc_feature    <10..>30"
    ));

    let complement = record_with_location(Location::span(-1, 9, 30));
    assert!(has_line(&text(&write(&complement)), "complement(10..30)"));

    let join = record_with_location(Location::join(
        1,
        vec![Segment::new(9, 30), Segment::new(49, 70)],
    ));
    assert!(has_line(&text(&write(&join)), "join(10..30,50..70)"));

    let order = record_with_location(Location::order(
        1,
        vec![
            Segment::new(0, 10),
            Segment::new(19, 30),
            Segment::new(0, 10),
        ],
    ));
    assert!(has_line(&text(&write(&order)), "order(1..10,20..30,1..10)"));

    let between = record_with_location(Location {
        operator: OPERATOR_SPAN.to_string(),
        strand: 1,
        segments: vec![Segment::with_boundaries(100, 100, false, false)],
    });
    assert!(has_line(&text(&write(&between)), "100^101"));
    let imported =
        import_genbank(&write(&between).bytes, &ImportOptions::default()).expect("between imports");
    let segment = &imported.records[0].features[0].location.segments[0];
    assert_eq!((segment.start, segment.end), (100, 100));
}

#[test]
fn unwrapped_origin_span_writes_one_range() {
    let record = one("040-near-origin");
    let location = &record.features[0].location;
    assert_eq!(location.operator, OPERATOR_SPAN);
    assert_eq!(location.segments.len(), 1);
    assert_eq!(
        (location.segments[0].start, location.segments[0].end),
        (5899, 6100)
    );
    assert!(location.segments[0].is_wrapped(record.length));

    let output = text(&write(&record));
    assert!(has_line(&output, "misc_feature    5900..6100"));
    assert!(!output.contains("join("));
}

#[test]
fn split_canonical_join_still_writes_join() {
    let mut record = one("040-near-origin");
    record.features[0].location =
        Location::join(1, vec![Segment::new(5899, 6000), Segment::new(0, 100)]);

    let output = write(&record);
    assert!(has_line(
        &text(&output),
        "misc_feature    join(5900..6000,1..100)"
    ));

    let imported =
        import_genbank(&output.bytes, &ImportOptions::default()).expect("split join re-imports");
    assert_eq!(
        imported.records[0].features[0].location,
        record.features[0].location
    );
}

#[test]
fn a_join_segment_that_wraps_writes_its_unwrapped_range() {
    let mut record = one("040-near-origin");
    record.features[0].location = Location::join(1, vec![Segment::new(5899, 6100)]);

    assert!(has_line(
        &text(&write(&record)),
        "misc_feature    join(5900..6100)"
    ));
    assert_round_trip(&record);
}

#[test]
fn near_origin_import_write_import_is_canonical_json_stable() {
    let record = one("040-near-origin");
    let first = to_canonical_json_bytes(&record).expect("canonical JSON");

    let imported =
        import_genbank(&write(&record).bytes, &ImportOptions::default()).expect("040 re-imports");
    assert_eq!(imported.records.len(), 1);
    let reimported = &imported.records[0];
    assert_equal_ignoring_provenance(&record, reimported);

    let mut comparable = reimported.clone();
    comparable.provenance = record.provenance.clone();
    let second = to_canonical_json_bytes(&comparable).expect("canonical JSON");
    assert_eq!(first, second, "040 canonical JSON survives the round trip");
}

#[test]
fn invalid_records_and_extras_are_typed_errors() {
    let record = one("001-minimal-cds");

    let mut zero_site = record.clone();
    zero_site.features[0].location = Location {
        operator: OPERATOR_SPAN.to_string(),
        strand: 1,
        segments: vec![Segment::with_boundaries(0, 0, false, false)],
    };
    let error = write_genbank(&zero_site, &WriteOptions::default()).expect_err("zero site");
    assert!(matches!(error, WriteError::InvalidRecord { .. }));

    let mut partial_site = record.clone();
    partial_site.features[0].location = Location {
        operator: OPERATOR_SPAN.to_string(),
        strand: 1,
        segments: vec![Segment::with_boundaries(100, 100, true, false)],
    };
    let error = write_genbank(&partial_site, &WriteOptions::default()).expect_err("partial site");
    assert!(matches!(error, WriteError::InvalidRecord { .. }));

    let mut multi_span = record.clone();
    multi_span.features[0].location = Location {
        operator: OPERATOR_SPAN.to_string(),
        strand: 1,
        segments: vec![Segment::new(1, 2), Segment::new(3, 4)],
    };
    let error = write_genbank(&multi_span, &WriteOptions::default()).expect_err("multi span");
    assert!(matches!(error, WriteError::InvalidRecord { .. }));

    let mut bad_division = record.clone();
    bad_division = with_header(&bad_division, &[], Some("XX"));
    let error = write_genbank(&bad_division, &WriteOptions::default()).expect_err("division");
    assert!(matches!(error, WriteError::InvalidRecord { .. }));

    let mut bad_header = record.clone();
    let mut extras = Extras::new();
    extras.insert("genbank", JsonValue::from("not an object"));
    bad_header.extras = extras;
    let error = write_genbank(&bad_header, &WriteOptions::default()).expect_err("header extras");
    assert!(matches!(error, WriteError::InvalidRecord { .. }));

    let mut newline_value = record.clone();
    newline_value.features[0].qualifiers[0].value = Some("a\nb".to_string());
    let error = write_genbank(&newline_value, &WriteOptions::default()).expect_err("newline value");
    assert!(matches!(error, WriteError::InvalidRecord { .. }));

    let mut bad_qualifier_key = record.clone();
    bad_qualifier_key.features[0].qualifiers[0].key = "bad key".to_string();
    let error = write_genbank(&bad_qualifier_key, &WriteOptions::default()).expect_err("qual key");
    assert!(matches!(error, WriteError::InvalidRecord { .. }));

    let mut bad_feature_key = record;
    bad_feature_key.features[0].key = "bad key".to_string();
    let error = write_genbank(&bad_feature_key, &WriteOptions::default()).expect_err("feature key");
    assert!(matches!(error, WriteError::InvalidRecord { .. }));
}

#[test]
fn output_is_ascii_and_diagnostics_are_sorted() {
    let mut record = one("001-minimal-cds");
    record.name = "THIS_NAME_IS_LONGER_THAN_SIXTEEN".to_string();
    record.features[0].qualifiers[0].value = Some("a\"b".to_string());
    record.features[0].location.strand = 0;
    let outcome = write(&record);
    assert!(outcome.bytes.is_ascii());
    let codes: Vec<&str> = outcome
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect();
    let mut sorted = codes.clone();
    sorted.sort_unstable();
    assert_eq!(codes, sorted);
}
