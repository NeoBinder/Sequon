mod common;

use common::{cds, make_record, qualifiers};
use sequon_algo::{
    analyze_record_with_database, analyze_selection, restriction_database, AnchorSide, CdsAnalysis,
    Selection, SelectionOperation, SelectionTranslationOptions,
};
use sequon_model::{Location, Segment, Topology};
use serde_json::Value;

const GOLDEN: &str = include_str!("../../../testdata/r11/r11-synthetic-analysis.json");

fn location(value: &Value) -> Location {
    Location {
        operator: value["operator"].as_str().expect("operator").to_owned(),
        strand: value["strand"].as_i64().expect("strand") as i8,
        segments: value["segments"]
            .as_array()
            .expect("segments")
            .iter()
            .map(|segment| {
                Segment::new(
                    segment["start"].as_u64().expect("start"),
                    segment["end"].as_u64().expect("end"),
                )
            })
            .collect(),
    }
}

fn feature_qualifiers(value: &Value) -> Vec<sequon_model::Qualifier> {
    let pairs: Vec<(&str, &str)> = value
        .as_array()
        .expect("qualifier array")
        .iter()
        .map(|qualifier| {
            (
                qualifier["key"].as_str().expect("qualifier key"),
                qualifier["value"].as_str().expect("qualifier value"),
            )
        })
        .collect();
    qualifiers(&pairs)
}

#[test]
fn r11_synthetic_6142bp_812aa_reverse_kanr_and_restriction_golden() {
    let root: Value = serde_json::from_str(GOLDEN).expect("golden JSON");
    assert_eq!(root["schema"], "sequon-analysis-golden/1");
    assert_eq!(root["fixture_class"], "r11-synthetic");
    let sequence = root["record"]["sequence"].as_str().expect("sequence");
    assert_eq!(sequence.len(), 6142);
    assert_eq!(root["record"]["length"], 6142);
    let features: Vec<_> = root["features"]
        .as_array()
        .expect("features")
        .iter()
        .map(|feature| {
            cds(
                location(&feature["location"]),
                feature_qualifiers(&feature["qualifiers"]),
            )
        })
        .collect();
    let record = make_record(sequence, Topology::Circular, features);
    let before = record.clone();
    let analysis =
        analyze_record_with_database(&record, &restriction_database()).expect("R11 analysis");
    assert_eq!(analysis.cds.len(), 2);

    let long = match &analysis.cds[0] {
        CdsAnalysis::Translation(translation) => translation,
        CdsAnalysis::Error(error) => panic!("long CDS failed: {}", error.message),
    };
    assert_eq!(long.source_length, 2436);
    assert_eq!(long.translated_length, 2436);
    assert_eq!(long.translated_codon_count, 812);
    assert_eq!(long.aa.len(), 812);
    assert_eq!(long.aa3.len(), 812);
    assert_eq!(long.aa.chars().next(), Some('M'));
    assert_eq!(long.aa.chars().last(), Some('A'));
    assert_eq!(long.aa3.first().map(String::as_str), Some("Met"));
    assert_eq!(long.aa3.last().map(String::as_str), Some("Ala"));
    assert_eq!(
        long.aa.chars().skip(1).filter(|base| *base == 'A').count(),
        811
    );
    assert_eq!(long.codons.len(), 812);
    assert_eq!(long.partial_tail_length, 0);

    let kanr = match &analysis.cds[1] {
        CdsAnalysis::Translation(translation) => translation,
        CdsAnalysis::Error(error) => panic!("reverse KanR failed: {}", error.message),
    };
    assert_eq!(kanr.aa, "A");
    assert_eq!(kanr.aa3, ["Ala"]);
    assert_eq!(kanr.codons[0].codon, "GCT");
    assert_eq!(
        (
            kanr.codons[0].segments[0].start,
            kanr.codons[0].segments[0].end
        ),
        (3000, 3003)
    );

    let selection = analyze_selection(
        &record,
        Selection::new(0, 3),
        &[
            SelectionOperation::Dna,
            SelectionOperation::ReverseComplement,
            SelectionOperation::Gc,
            SelectionOperation::Translation(SelectionTranslationOptions {
                strand: 1,
                frame: 0,
                transl_table: 1,
            }),
        ],
    )
    .expect("ATG selection");
    assert_eq!(selection.dna.as_deref(), Some("ATG"));
    assert_eq!(selection.revcomp.as_deref(), Some("CAT"));
    assert_eq!(selection.translation.as_ref().expect("AA").aa, "M");
    assert_eq!(selection.translation.as_ref().expect("AA3").aa3, ["Met"]);
    let gc = selection.gc.expect("GC");
    assert_eq!(
        (gc.count, gc.total, gc.percent_milli, gc.ambiguous_count),
        (1, 3, 33_333, 0)
    );

    let hits: Vec<_> = analysis
        .restriction_sites
        .iter()
        .filter(|hit| hit.enzyme_id == "BamHI")
        .collect();
    assert_eq!(hits.len(), 1);
    let hit = hits[0];
    assert_eq!(
        (
            hit.recognition.start,
            hit.recognition.end,
            hit.recognition.strand
        ),
        (5000, 5006, 0)
    );
    assert_eq!(
        hit.cuts
            .iter()
            .map(|cut| (cut.strand, cut.boundary))
            .collect::<Vec<_>>(),
        [(1, 5001), (-1, 5005)]
    );
    assert_eq!(
        (hit.anchor.start, hit.anchor.end, hit.anchor.side),
        (5001, 5002, AnchorSide::Right)
    );
    assert_eq!(record, before);
}
