#![cfg(feature = "snapgene")]

//! Corpus acceptance test for the SnapGene reader.
//!
//! Runs only when the gitignored corpus file exists: either under
//! `SEQUON_CORPUS_DIR` or at the repository-relative default path. The file is
//! read but never copied or committed, and the test asserts the facts recorded
//! in docs/design/snapgene-dna.md and the corpus findings note.

use std::path::PathBuf;

use sequon_io::api::{Format, ImportOptions};

const CORPUS_NAME: &str = "pET-28a(+).dna";

fn corpus_path() -> Option<PathBuf> {
    if let Ok(directory) = std::env::var("SEQUON_CORPUS_DIR") {
        let path = PathBuf::from(directory).join(CORPUS_NAME);
        return path.is_file().then_some(path);
    }
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("testdata")
        .join("corpus-local")
        .join(CORPUS_NAME);
    path.is_file().then_some(path)
}

fn import_corpus(bytes: &[u8]) -> sequon_io::api::ImportOutcome {
    let options = ImportOptions {
        format: Some(Format::Snapgene),
        ..ImportOptions::default()
    };
    sequon_io::import(bytes, &options).expect("corpus import must succeed")
}

#[test]
fn corpus_acceptance() {
    let Some(path) = corpus_path() else {
        eprintln!("corpus file not present; skipping the local corpus acceptance test");
        return;
    };
    let bytes = std::fs::read(&path).expect("read corpus file");
    assert_eq!(bytes.len(), 27646);
    let outcome = import_corpus(&bytes);
    assert_eq!(outcome.records.len(), 1);
    let record = &outcome.records[0];
    assert_eq!(record.name, "pET-28a(+)");
    assert_eq!(record.kind, sequon_model::Kind::Dna);
    assert_eq!(record.molecule_type, None);
    assert_eq!(record.topology, sequon_model::Topology::Circular);
    assert_eq!(record.length, 5369);
    assert_eq!(
        record.digests.sequence.as_str(),
        "sha256:00f6f8d5571b3f503417f87feb2cd8ed47700f3201be18e009366ea793f8901d"
    );
    assert_eq!(
        record.digests.canonical.as_str(),
        "sha256:621be0c246e44f046d0e7fde731f587255947af7fbb53031de3bbac01a77cfad"
    );
    assert_eq!(record.provenance.source_bytes, 27646);
    assert_eq!(
        record.provenance.source_digest.as_str(),
        "sha256:f298f4c87de87d193597771d63f515cb7906f8d71efb0ebc3d920e1a188f6a0d"
    );
    assert_eq!(record.provenance.export_version, Some(15));
    record.validate().expect("record must validate");
    sequon_algo::verify_digests(record).expect("digests must verify");

    let expected: [(&str, &str, i8, u64, u64); 16] = [
        ("misc_feature", "MCS", 1, 157, 203),
        ("CDS", "thrombin site", -1, 242, 260),
        ("RBS", "RBS", 1, 306, 312),
        ("CDS", "ATG", -1, 296, 299),
        ("CDS", "6xHis", -1, 269, 287),
        ("CDS", "6xHis", -1, 139, 157),
        ("promoter", "T7 promoter", -1, 367, 386),
        ("protein_bind", "lac operator", 1, 342, 367),
        ("CDS", "T7 tag", -1, 206, 239),
        ("terminator", "T7 terminator", 1, 25, 73),
        ("promoter", "lacI promoter", 1, 694, 772),
        ("CDS", "rop", 1, 2663, 2855),
        ("rep_origin", "f1 ori", -1, 4902, 5358),
        ("rep_origin", "ori", -1, 3284, 3873),
        ("CDS", "KanR", 1, 3994, 4810),
        ("CDS", "lacI", 1, 772, 1855),
    ];
    assert_eq!(record.features.len(), expected.len());
    for (feature, (key, name, strand, start, end)) in record.features.iter().zip(expected) {
        assert_eq!(feature.key, key);
        assert_eq!(feature.name.as_deref(), Some(name));
        assert_eq!(feature.location.strand, strand);
        assert_eq!(feature.location.operator, "span");
        assert_eq!(feature.location.segments.len(), 1);
        assert_eq!(feature.location.segments[0].start, start);
        assert_eq!(feature.location.segments[0].end, end);
    }

    // Notes are preserved in extras and give the name and description.
    let snapgene = record
        .extras
        .get("snapgene")
        .and_then(sequon_model::JsonValue::as_object)
        .expect("extras.snapgene");
    let notes = snapgene
        .get("notes")
        .and_then(sequon_model::JsonValue::as_array)
        .expect("extras.snapgene.notes");
    assert_eq!(notes.len(), 13);
    let custom_label = notes
        .iter()
        .find(|field| {
            field
                .as_object()
                .and_then(|object| object.get("name"))
                .and_then(sequon_model::JsonValue::as_str)
                == Some("CustomMapLabel")
        })
        .and_then(sequon_model::JsonValue::as_object)
        .and_then(|object| object.get("value"))
        .and_then(sequon_model::JsonValue::as_str);
    assert_eq!(custom_label, Some("pET-28a(+)"));
    let description = record.description.as_deref().unwrap_or("");
    assert!(description.contains("Bacterial vector for expression"));
    assert!(!description.contains("<html>"));

    // Only the two known-but-unmapped metadata blocks report info; no loss.
    let codes: Vec<&str> = outcome
        .report
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect();
    assert_eq!(codes, vec!["DNA_PACKET_IGNORED", "DNA_PACKET_IGNORED"]);
    assert_eq!(
        outcome.report.diagnostics[0].severity,
        sequon_io::report::Severity::Info
    );
    assert!(outcome.report.dropped.is_empty());
    assert_eq!(outcome.report.preserved.len(), 2);
    assert_eq!(outcome.report.preserved[0].what, "restriction_site_list");
    assert_eq!(outcome.report.preserved[0].offset, Some(5394));
    assert_eq!(outcome.report.preserved[1].what, "enzyme_settings");
    assert_eq!(outcome.report.preserved[1].offset, Some(26521));
    assert_eq!(outcome.report.export_version, Some(15));
    assert_eq!(outcome.report.records.len(), 1);
}
