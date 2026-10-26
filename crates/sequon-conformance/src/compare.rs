//! Cross-format comparison between a SnapGene `.dna` record and a GenBank record.
//!
//! This module implements deliverable C9 and the plan section 7.5 gate: it
//! compares two canonical records that are supposed to describe the same
//! molecule and reports, per dimension, whether the documented threshold is
//! met. It never edits a record, never re-parses a source format and never
//! hides a difference: every mismatch is listed with a coarse category and a
//! flag saying whether it caused a threshold failure.
//!
//! Dimensions and thresholds (plan section 7.5):
//!
//! | Dimension | Threshold | Rule |
//! | --- | --- | --- |
//! | sequence | 100% | normalized sequence equality and sequence-digest equality; when the topology matches, the canonical digest must match too |
//! | topology | 100% | `linear`/`circular` must be equal |
//! | features | Jaccard >= 0.99 | multiset overlap (duplicates count) of `(key, first.start, last.end, strand)` after origin-resolution of circular segments; a differing duplicate count is itself reported as a difference |
//! | qualifiers | exact | per matched feature, multiset equality after documented normalization (trim, quote-escape reversal, outer quote removal, whitespace collapse, case-folded values) |
//! | primers | 100% | equal `primer_bind` counts |
//!
//! The coarse feature tuple is the pass/fail metric (lead decision T6). A
//! segment-set-aware comparison is reported as an informational secondary
//! metric and is not a gate. Any difference, even an informational one, makes
//! the overall report fail so it is reviewed instead of being silently
//! accepted. Duplicate features are compared as a multiset: set semantics would
//! let an extra duplicate cancel out and report a false pass.
//!
//! The module is pure: it takes two records and returns a report. Importing
//! files, `Record::validate()`, `sequon_algo::verify_digests()`, JSON output and exit
//! codes belong to the `sequon-conformance` binary and its tests.

use std::collections::{BTreeMap, BTreeSet};

use sequon_model::{Feature, Record, Topology};
use serde::Serialize;

/// Comparison options.
#[derive(Debug, Clone, PartialEq)]
pub struct CompareOptions {
    /// Minimum feature Jaccard for the feature threshold (default 0.99).
    pub feature_threshold: f64,
}

impl Default for CompareOptions {
    fn default() -> Self {
        Self {
            feature_threshold: 0.99,
        }
    }
}

impl CompareOptions {
    /// Builds options with the given feature threshold.
    ///
    /// Returns `None` when the value is not finite or is outside `0.0..=1.0`.
    #[must_use]
    pub fn new(feature_threshold: f64) -> Option<Self> {
        if feature_threshold.is_finite() && (0.0..=1.0).contains(&feature_threshold) {
            Some(Self { feature_threshold })
        } else {
            None
        }
    }
}

/// Coarse feature identity used by the pass/fail Jaccard metric.
///
/// `start` is the first origin-resolved segment start and `end` the last
/// origin-resolved segment end, so a wrapped circular span and its explicitly
/// split representation compare equal.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct FeatureKey {
    /// Feature key exactly as the source wrote it.
    pub key: String,
    /// 0-based half-open start of the first resolved segment.
    pub start: u64,
    /// 0-based half-open end of the last resolved segment.
    pub end: u64,
    /// -1 reverse, 0 unstranded, 1 forward.
    pub strand: i8,
}

/// One origin-resolved segment boundary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct SegmentBoundary {
    /// 0-based half-open start.
    pub start: u64,
    /// 0-based half-open end.
    pub end: u64,
    /// True when the source marked a partial start.
    pub start_partial: bool,
    /// True when the source marked a partial end.
    pub end_partial: bool,
}

/// Segment-set-aware feature identity, used only for the informational
/// secondary metric.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct SegmentSignature {
    /// Feature key.
    pub key: String,
    /// -1 reverse, 0 unstranded, 1 forward.
    pub strand: i8,
    /// Origin-resolved segment boundaries in source order.
    pub segments: Vec<SegmentBoundary>,
}

/// How one qualifier differs between the two records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualifierDifferenceKind {
    /// Present in the `.dna` record but not in the GenBank record.
    MissingInGenbank,
    /// Present in the GenBank record but not in the `.dna` record.
    MissingInDna,
    /// Present in both, with different normalized values.
    ValueMismatch,
}

/// One qualifier difference for a matched feature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QualifierDifference {
    /// The feature the qualifier belongs to.
    pub feature: FeatureKey,
    /// Qualifier key.
    pub qualifier: String,
    /// Difference kind.
    pub kind: QualifierDifferenceKind,
    /// Normalized `.dna` value; `null` for a missing entry.
    pub dna_value: Option<String>,
    /// Normalized GenBank value; `null` for a missing entry.
    pub gb_value: Option<String>,
}

/// Coarse difference category (matching the register vocabulary in
/// `testdata/README.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DifferenceCategory {
    /// Sequence identity.
    Sequence,
    /// Topology identity.
    Topology,
    /// Feature set or segment set.
    FeatureSet,
    /// Qualifier content.
    Qualifier,
    /// Primer coverage.
    Primer,
}

impl DifferenceCategory {
    /// Returns the canonical label used by `testdata/README.md` and by the JSON
    /// report.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sequence => "SEQUENCE",
            Self::Topology => "TOPOLOGY",
            Self::FeatureSet => "FEATURE_SET",
            Self::Qualifier => "QUALIFIER",
            Self::Primer => "PRIMER",
        }
    }
}

/// One reported difference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Difference {
    /// Coarse category.
    pub category: DifferenceCategory,
    /// Human-readable detail; never used as identity.
    pub detail: String,
    /// True when this difference caused a documented threshold failure.
    pub threshold_failure: bool,
}

/// Sequence comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SequenceComparison {
    /// Record length from the `.dna` file.
    pub dna_length: u64,
    /// Record length from the GenBank file.
    pub gb_length: u64,
    /// Lengths are equal.
    pub lengths_equal: bool,
    /// Sequence strings are equal after normalization.
    pub normalized_equal: bool,
    /// Direction-sensitive sequence digests are equal.
    pub sequence_digest_equal: bool,
    /// Canonical digests are equal, or `null` when the topology differs and the
    /// comparison would be meaningless.
    pub canonical_digest_equal: Option<bool>,
}

/// Topology comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TopologyComparison {
    /// Topology from the `.dna` record.
    pub dna: String,
    /// Topology from the GenBank record.
    pub gb: String,
    /// Both topologies are equal.
    pub equal: bool,
}

/// Informational segment-set comparison.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SegmentSetComparison {
    /// Matching segment signatures.
    pub matched: usize,
    /// Union of segment signatures.
    pub union: usize,
    /// Segment-set Jaccard.
    pub jaccard: f64,
    /// Segment sets are identical.
    pub same: bool,
}

/// Feature comparison.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FeatureComparison {
    /// Feature count in the `.dna` record.
    pub dna_count: usize,
    /// Feature count in the GenBank record.
    pub gb_count: usize,
    /// Coarse keys present in both records.
    pub matched: usize,
    /// Union of coarse keys.
    pub union: usize,
    /// Coarse-key Jaccard (the pass/fail metric).
    pub jaccard: f64,
    /// Configured threshold.
    pub threshold: f64,
    /// Jaccard meets the threshold.
    pub passed: bool,
    /// Informational segment-set metric.
    pub segment_set: SegmentSetComparison,
    /// Coarse keys present only in the `.dna` record.
    pub missing: Vec<FeatureKey>,
    /// Coarse keys present only in the GenBank record.
    pub extra: Vec<FeatureKey>,
}

/// Qualifier comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QualifierComparison {
    /// Features paired by coarse key and compared.
    pub compared_features: usize,
    /// Per-feature qualifier differences.
    pub differences: Vec<QualifierDifference>,
    /// No differences.
    pub passed: bool,
}

/// Primer coverage comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PrimerComparison {
    /// `primer_bind` feature count in the `.dna` record.
    pub dna_count: usize,
    /// `primer_bind` feature count in the GenBank record.
    pub gb_count: usize,
    /// Counts are equal.
    pub equal: bool,
    /// Primer threshold is met.
    pub passed: bool,
}

/// Per-dimension threshold results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Thresholds {
    /// Sequence threshold.
    pub sequence: bool,
    /// Topology threshold.
    pub topology: bool,
    /// Feature Jaccard threshold.
    pub features: bool,
    /// Qualifier threshold.
    pub qualifiers: bool,
    /// Primer threshold.
    pub primers: bool,
    /// All thresholds pass.
    pub passed: bool,
}

/// Complete comparison report.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CompareReport {
    /// Sequence comparison.
    pub sequence: SequenceComparison,
    /// Topology comparison.
    pub topology: TopologyComparison,
    /// Feature comparison.
    pub features: FeatureComparison,
    /// Qualifier comparison.
    pub qualifiers: QualifierComparison,
    /// Primer comparison.
    pub primers: PrimerComparison,
    /// Every difference, in category order.
    pub differences: Vec<Difference>,
    /// Per-dimension threshold results.
    pub thresholds: Thresholds,
    /// True only when every threshold passes and no difference was reported.
    pub passed: bool,
}

/// Compares two validated records and returns a complete report.
///
/// The caller is responsible for importing exactly one record per file and for
/// running `Record::validate()` and `sequon_algo::verify_digests()` on both;
/// this function never re-imports or repairs a record. It is deterministic: the
/// same two records always produce the same report, and no message carries raw
/// sequence content.
#[must_use]
pub fn compare_records(dna: &Record, gb: &Record, options: &CompareOptions) -> CompareReport {
    let mut differences = Vec::new();
    let sequence = compare_sequence(dna, gb, &mut differences);
    let topology = compare_topology(dna, gb, &mut differences);
    let features = compare_features(dna, gb, options.feature_threshold, &mut differences);
    let qualifiers = compare_qualifiers(dna, gb, &mut differences);
    let primers = compare_primers(dna, gb, &mut differences);

    let thresholds = Thresholds {
        sequence: sequence.lengths_equal
            && sequence.normalized_equal
            && sequence.sequence_digest_equal
            && sequence.canonical_digest_equal != Some(false),
        topology: topology.equal,
        features: features.passed,
        qualifiers: qualifiers.passed,
        primers: primers.passed,
        passed: false,
    };
    let thresholds = Thresholds {
        passed: thresholds.sequence
            && thresholds.topology
            && thresholds.features
            && thresholds.qualifiers
            && thresholds.primers,
        ..thresholds
    };
    let passed = thresholds.passed && differences.is_empty();
    CompareReport {
        sequence,
        topology,
        features,
        qualifiers,
        primers,
        differences,
        thresholds,
        passed,
    }
}

fn compare_sequence(
    dna: &Record,
    gb: &Record,
    differences: &mut Vec<Difference>,
) -> SequenceComparison {
    let lengths_equal = dna.length == gb.length;
    if !lengths_equal {
        differences.push(Difference {
            category: DifferenceCategory::Sequence,
            detail: format!(
                "record lengths differ: .dna {} vs GenBank {}",
                dna.length, gb.length
            ),
            threshold_failure: true,
        });
    }

    let dna_sequence = normalize_sequence(&dna.sequence);
    let gb_sequence = normalize_sequence(&gb.sequence);
    let normalized_equal = dna_sequence == gb_sequence;
    if !normalized_equal {
        differences.push(Difference {
            category: DifferenceCategory::Sequence,
            detail: "normalized sequences differ (content intentionally not echoed)".to_owned(),
            threshold_failure: true,
        });
    }

    let sequence_digest_equal = dna.digests.sequence == gb.digests.sequence;
    if !sequence_digest_equal {
        differences.push(Difference {
            category: DifferenceCategory::Sequence,
            detail: "sequence digest differs".to_owned(),
            threshold_failure: true,
        });
    }

    let topology_equal = dna.topology == gb.topology;
    let canonical_digest_equal = if topology_equal {
        Some(dna.digests.canonical == gb.digests.canonical)
    } else {
        None
    };
    if canonical_digest_equal == Some(false) {
        differences.push(Difference {
            category: DifferenceCategory::Sequence,
            detail: "canonical digest differs although topology matches".to_owned(),
            threshold_failure: true,
        });
    }

    SequenceComparison {
        dna_length: dna.length,
        gb_length: gb.length,
        lengths_equal,
        normalized_equal,
        sequence_digest_equal,
        canonical_digest_equal,
    }
}

fn compare_topology(
    dna: &Record,
    gb: &Record,
    differences: &mut Vec<Difference>,
) -> TopologyComparison {
    let equal = dna.topology == gb.topology;
    if !equal {
        differences.push(Difference {
            category: DifferenceCategory::Topology,
            detail: format!(
                "topology differs: .dna {} vs GenBank {}",
                topology_label(dna.topology),
                topology_label(gb.topology)
            ),
            threshold_failure: true,
        });
    }
    TopologyComparison {
        dna: topology_label(dna.topology).to_owned(),
        gb: topology_label(gb.topology).to_owned(),
        equal,
    }
}

fn compare_features(
    dna: &Record,
    gb: &Record,
    threshold: f64,
    differences: &mut Vec<Difference>,
) -> FeatureComparison {
    // Counts, not sets: duplicate features are a real difference and must not
    // cancel out against their single counterpart in the other record.
    let dna_counts = feature_counts(dna);
    let gb_counts = feature_counts(gb);
    let mut keys: BTreeSet<&FeatureKey> = dna_counts.keys().collect();
    keys.extend(gb_counts.keys());

    let mut matched = 0_usize;
    let mut union = 0_usize;
    let mut missing: Vec<FeatureKey> = Vec::new();
    let mut extra: Vec<FeatureKey> = Vec::new();
    let mut duplicates: Vec<(FeatureKey, usize, usize)> = Vec::new();
    for key in keys {
        let dna_count = dna_counts.get(key).copied().unwrap_or(0);
        let gb_count = gb_counts.get(key).copied().unwrap_or(0);
        matched += dna_count.min(gb_count);
        union += dna_count.max(gb_count);
        if dna_count == gb_count {
            continue;
        }
        duplicates.push((key.clone(), dna_count, gb_count));
        for _ in gb_count..dna_count {
            missing.push(key.clone());
        }
        for _ in dna_count..gb_count {
            extra.push(key.clone());
        }
    }

    let feature_jaccard = jaccard(matched, union);
    let passed = feature_jaccard >= threshold;

    for (key, dna_count, gb_count) in &duplicates {
        differences.push(Difference {
            category: DifferenceCategory::FeatureSet,
            detail: format!(
                "feature {key:?} appears {dna_count} time(s) in .dna and {gb_count} time(s) in GenBank"
            ),
            threshold_failure: !passed,
        });
    }
    for feature in &missing {
        differences.push(Difference {
            category: DifferenceCategory::FeatureSet,
            detail: format!("feature missing from GenBank: {feature:?}"),
            threshold_failure: !passed,
        });
    }
    for feature in &extra {
        differences.push(Difference {
            category: DifferenceCategory::FeatureSet,
            detail: format!("feature missing from .dna: {feature:?}"),
            threshold_failure: !passed,
        });
    }
    if !passed {
        differences.push(Difference {
            category: DifferenceCategory::FeatureSet,
            detail: format!(
                "feature Jaccard {feature_jaccard:.4} is below the threshold {threshold:.4} ({matched}/{union})"
            ),
            threshold_failure: true,
        });
    }

    let dna_segments = segment_counts(dna);
    let gb_segments = segment_counts(gb);
    let mut segment_keys: BTreeSet<&SegmentSignature> = dna_segments.keys().collect();
    segment_keys.extend(gb_segments.keys());
    let mut segment_matched = 0_usize;
    let mut segment_union = 0_usize;
    for key in segment_keys {
        let dna_count = dna_segments.get(key).copied().unwrap_or(0);
        let gb_count = gb_segments.get(key).copied().unwrap_or(0);
        segment_matched += dna_count.min(gb_count);
        segment_union += dna_count.max(gb_count);
    }
    let segment_jaccard = jaccard(segment_matched, segment_union);
    let segment_same = dna_segments == gb_segments;
    if !segment_same {
        differences.push(Difference {
            category: DifferenceCategory::FeatureSet,
            detail: format!(
                "segment-set Jaccard {segment_jaccard:.4} differs from the coarse mapping (informational, not a gate)"
            ),
            threshold_failure: false,
        });
    }

    FeatureComparison {
        dna_count: dna.features.len(),
        gb_count: gb.features.len(),
        matched,
        union,
        jaccard: feature_jaccard,
        threshold,
        passed,
        segment_set: SegmentSetComparison {
            matched: segment_matched,
            union: segment_union,
            jaccard: segment_jaccard,
            same: segment_same,
        },
        missing,
        extra,
    }
}

fn compare_qualifiers(
    dna: &Record,
    gb: &Record,
    differences: &mut Vec<Difference>,
) -> QualifierComparison {
    let dna_features = features_by_key(dna);
    let gb_features = features_by_key(gb);
    let mut qualifier_differences = Vec::new();
    let mut compared_features = 0_usize;

    for (key, dna_group) in &dna_features {
        let Some(gb_group) = gb_features.get(key) else {
            continue;
        };
        // Duplicate coarse keys are paired by qualifier content, not by source
        // order, so two records that list the same duplicates in a different
        // order do not produce spurious differences.
        let mut dna_group = dna_group.clone();
        let mut gb_group = gb_group.clone();
        dna_group.sort_by_cached_key(|feature| qualifier_values(feature));
        gb_group.sort_by_cached_key(|feature| qualifier_values(feature));
        for (dna_feature, gb_feature) in dna_group.iter().zip(gb_group.iter()) {
            compared_features += 1;
            qualifier_differences.extend(difference_in_qualifiers(key, dna_feature, gb_feature));
        }
    }

    for difference in &qualifier_differences {
        differences.push(Difference {
            category: DifferenceCategory::Qualifier,
            detail: format!(
                "qualifier '{}' on feature {:?} differs ({:?})",
                difference.qualifier, difference.feature, difference.kind
            ),
            threshold_failure: true,
        });
    }

    QualifierComparison {
        compared_features,
        passed: qualifier_differences.is_empty(),
        differences: qualifier_differences,
    }
}

fn compare_primers(
    dna: &Record,
    gb: &Record,
    differences: &mut Vec<Difference>,
) -> PrimerComparison {
    let dna_count = primer_count(dna);
    let gb_count = primer_count(gb);
    let equal = dna_count == gb_count;
    if !equal {
        differences.push(Difference {
            category: DifferenceCategory::Primer,
            detail: format!("primer_bind count differs: .dna {dna_count} vs GenBank {gb_count}"),
            threshold_failure: true,
        });
    }
    PrimerComparison {
        dna_count,
        gb_count,
        equal,
        passed: equal,
    }
}

fn difference_in_qualifiers(
    key: &FeatureKey,
    dna_feature: &Feature,
    gb_feature: &Feature,
) -> Vec<QualifierDifference> {
    let dna_values = qualifier_values(dna_feature);
    let gb_values = qualifier_values(gb_feature);
    let keys: BTreeSet<&String> = dna_values.keys().chain(gb_values.keys()).collect();
    let mut differences = Vec::new();
    for qualifier in keys {
        let empty = Vec::new();
        let dna_list = dna_values.get(qualifier).unwrap_or(&empty);
        let gb_list = gb_values.get(qualifier).unwrap_or(&empty);
        let mut dna_left = dna_list.clone();
        let mut gb_left = gb_list.clone();
        dna_left.retain(
            |value| match gb_left.iter().position(|candidate| candidate == value) {
                Some(position) => {
                    gb_left.remove(position);
                    false
                }
                None => true,
            },
        );
        let mismatched = dna_left.len().min(gb_left.len());
        for index in 0..mismatched {
            differences.push(QualifierDifference {
                feature: key.clone(),
                qualifier: qualifier.clone(),
                kind: QualifierDifferenceKind::ValueMismatch,
                dna_value: dna_left[index].clone(),
                gb_value: gb_left[index].clone(),
            });
        }
        for value in dna_left.iter().skip(mismatched) {
            differences.push(QualifierDifference {
                feature: key.clone(),
                qualifier: qualifier.clone(),
                kind: QualifierDifferenceKind::MissingInGenbank,
                dna_value: value.clone(),
                gb_value: None,
            });
        }
        for value in gb_left.iter().skip(mismatched) {
            differences.push(QualifierDifference {
                feature: key.clone(),
                qualifier: qualifier.clone(),
                kind: QualifierDifferenceKind::MissingInDna,
                dna_value: None,
                gb_value: value.clone(),
            });
        }
    }
    differences
}

fn qualifier_values(feature: &Feature) -> BTreeMap<String, Vec<Option<String>>> {
    let mut values: BTreeMap<String, Vec<Option<String>>> = BTreeMap::new();
    for qualifier in &feature.qualifiers {
        values
            .entry(qualifier.key.clone())
            .or_default()
            .push(normalize_qualifier_value(qualifier.value.as_deref()));
    }
    values
}

fn feature_counts(record: &Record) -> BTreeMap<FeatureKey, usize> {
    let mut counts: BTreeMap<FeatureKey, usize> = BTreeMap::new();
    for feature in &record.features {
        *counts
            .entry(feature_key(feature, record.length))
            .or_insert(0) += 1;
    }
    counts
}

fn segment_counts(record: &Record) -> BTreeMap<SegmentSignature, usize> {
    let mut counts: BTreeMap<SegmentSignature, usize> = BTreeMap::new();
    for feature in &record.features {
        *counts
            .entry(segment_signature(feature, record.length))
            .or_insert(0) += 1;
    }
    counts
}

fn feature_key(feature: &Feature, length: u64) -> FeatureKey {
    let resolved = feature.location.resolved_segments(length);
    let start = resolved.first().map_or(0, |segment| segment.start);
    let end = resolved.last().map_or(0, |segment| segment.end);
    FeatureKey {
        key: feature.key.clone(),
        start,
        end,
        strand: feature.location.strand,
    }
}

fn segment_signature(feature: &Feature, length: u64) -> SegmentSignature {
    let segments = feature
        .location
        .resolved_segments(length)
        .into_iter()
        .map(|segment| SegmentBoundary {
            start: segment.start,
            end: segment.end,
            start_partial: segment.start_partial,
            end_partial: segment.end_partial,
        })
        .collect();
    SegmentSignature {
        key: feature.key.clone(),
        strand: feature.location.strand,
        segments,
    }
}

fn features_by_key(record: &Record) -> BTreeMap<FeatureKey, Vec<&Feature>> {
    let mut groups: BTreeMap<FeatureKey, Vec<&Feature>> = BTreeMap::new();
    for feature in &record.features {
        groups
            .entry(feature_key(feature, record.length))
            .or_default()
            .push(feature);
    }
    groups
}

fn primer_count(record: &Record) -> usize {
    record
        .features
        .iter()
        .filter(|feature| feature.key.eq_ignore_ascii_case("primer_bind"))
        .count()
}

fn topology_label(topology: Topology) -> &'static str {
    match topology {
        Topology::Linear => "linear",
        Topology::Circular => "circular",
    }
}

fn jaccard(matched: usize, union: usize) -> f64 {
    if union == 0 {
        1.0
    } else {
        matched as f64 / union as f64
    }
}

fn normalize_sequence(sequence: &str) -> String {
    sequence
        .chars()
        .filter(|character| !character.is_whitespace() && !character.is_ascii_digit())
        .map(|character| character.to_ascii_uppercase())
        .collect()
}

/// Normalizes one qualifier value for comparison.
///
/// The documented normalization (testdata/README.md section 8.3) is: trim,
/// reverse the writer's quote escape, remove outer quotes, collapse whitespace
/// and case-fold. Reversing two apostrophes to a double quote is deliberately
/// ambiguous: a value that genuinely contains two apostrophes cannot be told
/// apart from an escaped double quote. This is the accepted trade-off that lets
/// a canonical record compare equal to its own GenBank writer output, which
/// replaces every double quote with two apostrophes and reports
/// WRITE_QUOTE_NORMALIZED.
fn normalize_qualifier_value(value: Option<&str>) -> Option<String> {
    value.map(|raw| {
        let trimmed = raw.trim();
        let reversed = trimmed.replace("''", "\"");
        let unquoted = strip_outer_quotes(&reversed);
        unquoted
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    })
}

fn strip_outer_quotes(value: &str) -> &str {
    if let Some(inner) = value
        .strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
    {
        inner
    } else if let Some(inner) = value
        .strip_prefix('\'')
        .and_then(|text| text.strip_suffix('\''))
    {
        inner
    } else {
        value
    }
}
