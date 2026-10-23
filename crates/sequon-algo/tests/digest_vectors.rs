//! Digest vectors for sequon-digest/1.
//!
//! The vector file lives at the repository root in tests/digest-vectors.json.
//! Its expected values were computed independently with Python hashlib from the
//! equivalence-class definition in docs/design/canonical-model.md, not derived
//! from this implementation. The test also asserts the negative expectations:
//! reverse complementing or rotating a sequence changes the sequence digest but
//! keeps the canonical digest.

use sequon_algo::{
    canonical_digest, check_normalized, normalize_sequence, reverse_complement, sequence_digest,
    IUPAC_DNA,
};
use sequon_model::{Topology, DIGEST_ALGORITHM_V1};

const VECTORS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/digest-vectors.json"
));

fn text<'a>(value: &'a serde_json::Value, key: &str) -> &'a str {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("field '{key}' must be a JSON string"))
}

fn optional<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(serde_json::Value::as_str)
}

fn cases<'a>(vectors: &'a serde_json::Value, section: &str) -> &'a Vec<serde_json::Value> {
    vectors
        .get(section)
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("section {section} must be a JSON array"))
}

#[test]
fn vector_file_declares_the_accepted_algorithm() {
    let vectors: serde_json::Value = serde_json::from_str(VECTORS).expect("vectors must be JSON");
    assert_eq!(text(&vectors, "algorithm"), DIGEST_ALGORITHM_V1);
    assert_eq!(text(&vectors, "algorithm"), "sequon-digest/1");
}

#[test]
fn every_vector_matches_independent_expectations() {
    let vectors: serde_json::Value = serde_json::from_str(VECTORS).expect("vectors must be JSON");
    for (section, topology) in [
        ("linear", Topology::Linear),
        ("circular", Topology::Circular),
    ] {
        let section_cases = cases(&vectors, section);
        assert!(
            section_cases.len() >= 4,
            "section {section} needs at least 4 cases, found {}",
            section_cases.len()
        );

        for case in section_cases {
            let name = text(case, "name");
            let input = text(case, "input");
            let normalized = text(case, "normalized");
            let expected_sequence = text(case, "sequence_digest");
            let expected_canonical = text(case, "canonical_digest");

            assert!(
                normalized
                    .chars()
                    .all(|character| IUPAC_DNA.contains(character)),
                "{name}: the normalized vector must be uppercase IUPAC DNA"
            );
            check_normalized(normalized).unwrap_or_else(|error| {
                panic!("{name}: normalized vector is not normalized: {error}")
            });

            let actual_normalized = normalize_sequence(input)
                .unwrap_or_else(|error| panic!("{name}: normalize_sequence failed: {error}"));
            assert_eq!(actual_normalized, normalized, "{name}: normalization");

            let actual_sequence = sequence_digest(normalized)
                .unwrap_or_else(|error| panic!("{name}: sequence_digest failed: {error}"));
            assert_eq!(
                actual_sequence.as_str(),
                expected_sequence,
                "{name}: sequence digest"
            );

            let actual_canonical = canonical_digest(normalized, topology)
                .unwrap_or_else(|error| panic!("{name}: canonical_digest failed: {error}"));
            assert_eq!(
                actual_canonical.as_str(),
                expected_canonical,
                "{name}: canonical digest"
            );

            // Negative expectation: the other orientation keeps the canonical
            // digest and changes the sequence digest.
            let reverse = reverse_complement(normalized)
                .unwrap_or_else(|error| panic!("{name}: reverse_complement failed: {error}"));
            let reverse_canonical = canonical_digest(&reverse, topology)
                .unwrap_or_else(|error| panic!("{name}: reverse canonical_digest failed: {error}"));
            assert_eq!(
                reverse_canonical.as_str(),
                expected_canonical,
                "{name}: reverse complement must keep the canonical digest"
            );
            let reverse_sequence = sequence_digest(&reverse)
                .unwrap_or_else(|error| panic!("{name}: reverse sequence_digest failed: {error}"));
            assert_ne!(
                reverse_sequence.as_str(),
                expected_sequence,
                "{name}: reverse complement must change the sequence digest"
            );
        }
    }
}

#[test]
fn equivalence_pairs_keep_the_canonical_digest_and_change_the_sequence_digest() {
    let vectors: serde_json::Value = serde_json::from_str(VECTORS).expect("vectors must be JSON");
    let mut by_name = std::collections::HashMap::new();
    for section in ["linear", "circular"] {
        for case in cases(&vectors, section) {
            by_name.insert(text(case, "name"), case.clone());
        }
    }

    for (name, case) in &by_name {
        if let Some(other_name) = optional(case, "same_canonical_as") {
            let other = by_name
                .get(other_name)
                .unwrap_or_else(|| panic!("{name} references missing case {other_name}"));
            assert_eq!(
                text(case, "canonical_digest"),
                text(other, "canonical_digest"),
                "{name} and {other_name} must share the canonical digest"
            );
        }
        if let Some(other_name) = optional(case, "sequence_differs_from") {
            let other = by_name
                .get(other_name)
                .unwrap_or_else(|| panic!("{name} references missing case {other_name}"));
            assert_ne!(
                text(case, "sequence_digest"),
                text(other, "sequence_digest"),
                "{name} and {other_name} must have different sequence digests"
            );
        }
    }
}

#[test]
fn the_normalization_vector_actually_changes_the_text() {
    let vectors: serde_json::Value = serde_json::from_str(VECTORS).expect("vectors must be JSON");
    let mut found = false;
    for section in ["linear", "circular"] {
        for case in cases(&vectors, section) {
            if text(case, "name").contains("normalized") {
                found = true;
                assert_ne!(
                    text(case, "input"),
                    text(case, "normalized"),
                    "the normalization vector must exercise lowercase, whitespace or digits"
                );
            }
        }
    }
    assert!(found, "the vector file must carry a normalization case");
}
