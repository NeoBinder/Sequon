mod common;

use common::{cds, make_record, qualifiers};
use sequon_algo::{
    analyze_record_with_database, analyze_restriction, analyze_selection, gc_content,
    genetic_code_database, restriction_database, translate_cds, translate_selection, AnalysisError,
    AnchorSide, CdsAnalysis, Continuity, OverhangKind, RestrictionAmbiguityCode,
    RestrictionDatabase, RestrictionEnzyme, Selection, SelectionOperation,
    SelectionTranslationOptions,
};
use sequon_model::{Location, Segment, Topology};

#[test]
fn extraction_preserves_join_order_and_whole_location_negative_strand() {
    let record = make_record("AAACCCGGGTTT", Topology::Circular, vec![]);
    let join = Location::join(-1, vec![Segment::new(0, 3), Segment::new(6, 9)]);
    let extracted = sequon_algo::extract_location(&record, &join).expect("join extraction");
    assert_eq!(extracted.dna, "CCCTTT");
    assert_eq!(extracted.continuity, Continuity::Joined);
    assert_eq!(extracted.segments.len(), 2);
    assert_eq!(
        (extracted.segments[0].start, extracted.segments[0].end),
        (6, 9)
    );
    assert_eq!(extracted.segments[0].dna, "CCC");
    assert_eq!(
        (extracted.segments[1].start, extracted.segments[1].end),
        (0, 3)
    );
    assert_eq!(extracted.segments[1].dna, "TTT");

    let order = Location::order(1, vec![Segment::new(0, 3), Segment::new(6, 9)]);
    let extracted = sequon_algo::extract_location(&record, &order).expect("order extraction");
    assert_eq!(extracted.dna, "AAAGGG");
    assert_eq!(extracted.continuity, Continuity::OrderedUnknownGaps);
}

#[test]
fn extraction_resolves_origin_before_applying_strand() {
    let record = make_record("ACGTTGCAACTG", Topology::Circular, vec![]);
    let forward = sequon_algo::extract_location(&record, &Location::span(1, 10, 14))
        .expect("origin selection");
    assert_eq!(forward.dna, "TGAC");
    assert_eq!(
        forward
            .segments
            .iter()
            .map(|s| s.dna.as_str())
            .collect::<Vec<_>>(),
        ["TG", "AC"]
    );
    let reverse = sequon_algo::extract_location(&record, &Location::span(-1, 10, 14))
        .expect("reverse origin selection");
    assert_eq!(reverse.dna, "GTCA");
    assert_eq!(
        reverse
            .segments
            .iter()
            .map(|s| s.dna.as_str())
            .collect::<Vec<_>>(),
        ["GT", "CA"]
    );
}

#[test]
fn selection_rejects_empty_and_out_of_range_coordinates() {
    let record = make_record("ATG", Topology::Linear, vec![]);
    assert!(matches!(
        sequon_algo::extract_selection(&record, Selection::new(1, 1)),
        Err(AnalysisError::SelectionEmpty)
    ));
    assert!(matches!(
        sequon_algo::extract_selection(&record, Selection::new(1, 4)),
        Err(AnalysisError::SelectionOutOfRange { .. })
    ));
    let site = sequon_algo::extract_location(&record, &Location::span(1, 1, 1))
        .expect("zero-length site is not a selection");
    assert!(site.dna.is_empty());
}

#[test]
fn selection_atg_gc_revcomp_and_translation_match_contract() {
    let record = make_record("ATG", Topology::Linear, vec![]);
    let result = analyze_selection(
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
    assert_eq!(result.dna.as_deref(), Some("ATG"));
    assert_eq!(result.revcomp.as_deref(), Some("CAT"));
    let gc = result.gc.expect("GC result");
    assert_eq!(
        (gc.count, gc.total, gc.percent_milli, gc.ambiguous_count),
        (1, 3, 33_333, 0)
    );
    let translation = result.translation.expect("translation");
    assert_eq!(translation.aa, "M");
    assert_eq!(translation.aa3, ["Met"]);
    assert_eq!(translation.codons[0].segments[0].start, 0);
    assert_eq!(translation.codons[0].segments[0].end, 3);
    assert!(result
        .provenance
        .iter()
        .any(|p| p.algorithm.id == "sequon-gc"));
    assert!(result
        .provenance
        .iter()
        .any(|p| p.algorithm.id == "sequon-translation"));
}

#[test]
fn reverse_selection_applies_revcomp_before_frame_and_validates_selection() {
    let record = make_record("CCAT", Topology::Linear, vec![]);
    let translated = translate_selection(
        &record,
        Selection::new(0, 4),
        SelectionTranslationOptions {
            strand: -1,
            frame: 1,
            transl_table: 1,
        },
    )
    .expect("reverse frame");
    assert_eq!(translated.aa, "W");
    assert_eq!(translated.codons[0].codon, "TGG");
    assert_eq!(translated.partial_tail_length, 0);
    assert!(matches!(
        translate_selection(
            &record,
            Selection::new(2, 2),
            SelectionTranslationOptions {
                strand: -1,
                frame: 0,
                transl_table: 1,
            },
        ),
        Err(AnalysisError::SelectionEmpty)
    ));
}

#[test]
fn gc_counts_only_certain_gc_symbols_and_rounds_exactly() {
    let gc = gc_content("GCSN").expect("valid IUPAC");
    assert_eq!(
        (gc.count, gc.total, gc.percent_milli, gc.ambiguous_count),
        (3, 4, 75_000, 2)
    );
    let gc = gc_content("ATG").expect("valid IUPAC");
    assert_eq!(gc.percent_milli, 33_333);
}

#[test]
fn translation_honors_codon_start_table_11_and_reverse_kanr_codon() {
    let feature = cds(
        Location::span(1, 0, 4),
        qualifiers(&[("codon_start", "2"), ("transl_table", "11")]),
    );
    let record = make_record("AATG", Topology::Linear, vec![feature]);
    let translated = translate_cds(&record, 0).expect("offset ATG");
    assert_eq!(translated.aa, "M");
    assert_eq!(translated.codon_start, 2);
    assert_eq!(translated.transl_table, 11);

    let feature = cds(Location::span(-1, 0, 3), vec![]);
    let record = make_record("AGC", Topology::Linear, vec![feature]);
    let translated = translate_cds(&record, 0).expect("reverse Ala");
    assert_eq!(translated.aa, "A");
    assert_eq!(translated.aa3, ["Ala"]);
    assert_eq!(translated.codons[0].codon, "GCT");
    assert_eq!(translated.codons[0].segments[0].start, 0);
    assert_eq!(translated.codons[0].segments[0].end, 3);
}

#[test]
fn translation_rejects_unsupported_table_and_order_cds() {
    let feature = cds(
        Location::span(1, 0, 3),
        qualifiers(&[("transl_table", "2")]),
    );
    let record = make_record("ATG", Topology::Linear, vec![feature]);
    assert!(matches!(
        translate_cds(&record, 0),
        Err(AnalysisError::TranslationTableUnsupported { table: 2 })
    ));

    let feature = cds(Location::order(1, vec![Segment::new(0, 3)]), vec![]);
    let record = make_record("ATG", Topology::Linear, vec![feature]);
    assert!(matches!(
        translate_cds(&record, 0),
        Err(AnalysisError::OrderCdsUnsupported)
    ));
}

#[test]
fn iupac_unanimity_stops_and_partial_tail_are_explicit() {
    let feature = cds(Location::span(1, 0, 17), vec![]);
    let record = make_record("GCNTARTGNATHATGAA", Topology::Linear, vec![feature]);
    let translated = translate_cds(&record, 0).expect("IUPAC translation");
    assert_eq!(translated.aa, "A*XIM");
    assert_eq!(translated.aa3, ["Ala", "Ter", "Xaa", "Ile", "Met"]);
    assert_eq!(translated.ambiguous_codon_count, 4);
    assert_eq!(translated.ambiguous_stop_codon_count, 1);
    assert_eq!(translated.stop_codon_count, 1);
    assert_eq!(translated.partial_tail_length, 2);
    assert_eq!(translated.partial_tail_dna, "AA");
    assert_eq!(translated.translated_length, 15);
}

#[test]
fn codon_mapping_preserves_origin_and_join_pieces_in_biological_order() {
    let feature = cds(
        Location::join(1, vec![Segment::new(10, 12), Segment::new(0, 4)]),
        vec![],
    );
    let record = make_record("ACGTTGCAACTG", Topology::Circular, vec![feature]);
    let translated = translate_cds(&record, 0).expect("joined translation");
    assert_eq!(translated.aa, "*R");
    assert_eq!(translated.codons[0].codon, "TGA");
    assert_eq!(
        translated.codons[0]
            .segments
            .iter()
            .map(|s| (s.start, s.end))
            .collect::<Vec<_>>(),
        [(10, 12), (0, 1)]
    );
    assert_eq!(
        translated.codons[1]
            .segments
            .iter()
            .map(|s| (s.start, s.end))
            .collect::<Vec<_>>(),
        [(1, 4)]
    );
}

#[test]
fn record_analysis_keeps_unsupported_cds_and_does_not_mutate_record() {
    let valid = cds(Location::span(1, 0, 3), vec![]);
    let unsupported = cds(Location::order(1, vec![Segment::new(3, 6)]), vec![]);
    let record = make_record("ATGATG", Topology::Linear, vec![valid, unsupported]);
    let before = record.clone();
    let analysis =
        analyze_record_with_database(&record, &restriction_database()).expect("record analysis");
    assert_eq!(analysis.cds.len(), 2);
    assert!(matches!(analysis.cds[0], CdsAnalysis::Translation(_)));
    assert!(matches!(
        &analysis.cds[1],
        CdsAnalysis::Error(error) if error.code == "ORDER_CDS_UNSUPPORTED" && error.feature_index == 1
    ));
    assert_eq!(record, before);
    assert!(analysis
        .provenance
        .iter()
        .any(|p| p.algorithm.id == "sequon-restriction"));
}

#[test]
fn restriction_bamhi_has_palindromic_dedup_cuts_anchor_and_provenance() {
    let record = make_record("AAAAGGATCCAAAA", Topology::Linear, vec![]);
    let analysis =
        analyze_restriction(&record, &restriction_database()).expect("restriction analysis");
    let hits: Vec<_> = analysis
        .hits
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
        (4, 10, 0)
    );
    assert_eq!(
        hit.cuts
            .iter()
            .map(|cut| (cut.strand, cut.boundary))
            .collect::<Vec<_>>(),
        [(1, 5), (-1, 9)]
    );
    assert_eq!(
        (hit.anchor.start, hit.anchor.end, hit.anchor.side),
        (5, 6, AnchorSide::Right)
    );
    assert_eq!(
        (hit.overhang.kind, hit.overhang.length),
        (OverhangKind::FivePrime, 4)
    );
    assert_eq!(analysis.provenance.algorithm.id, "sequon-restriction");
    let database = analysis
        .provenance
        .database
        .as_ref()
        .expect("DB provenance");
    assert_eq!(database.id, "sequon-restriction-enzymes");
    assert!(database.sha256.starts_with("sha256:"));
}

#[test]
fn restriction_separates_possible_iupac_intersection_from_confirmed_subset() {
    let record = make_record("AAAAGGATCNAAAA", Topology::Linear, vec![]);
    let analysis =
        analyze_restriction(&record, &restriction_database()).expect("ambiguous analysis");
    assert!(!analysis.hits.iter().any(|hit| hit.enzyme_id == "BamHI"));
    let ambiguity = analysis
        .ambiguities
        .iter()
        .find(|hit| hit.enzyme_id == "BamHI")
        .expect("possible BamHI");
    assert!(matches!(
        ambiguity.code,
        RestrictionAmbiguityCode::AmbiguousTemplate
    ));

    let database = RestrictionDatabase::new(
        "test-degenerate-db",
        "1",
        vec![RestrictionEnzyme::new("TEST-N", "ANNT", 1, 1)],
    )
    .expect("test DB");
    let record = make_record("AAAAASNTAAAA", Topology::Linear, vec![]);
    let analysis = analyze_restriction(&record, &database).expect("subset analysis");
    assert_eq!(analysis.hits.len(), 1);
    assert!(!analysis
        .ambiguities
        .iter()
        .any(|hit| hit.recognition.start == 4 && hit.recognition.end == 8));
}

#[test]
fn restriction_matches_origin_once_and_normalizes_cuts() {
    let record = make_record("ATCCAAAAAAGG", Topology::Circular, vec![]);
    let analysis = analyze_restriction(&record, &restriction_database()).expect("origin analysis");
    let hits: Vec<_> = analysis
        .hits
        .iter()
        .filter(|hit| hit.enzyme_id == "BamHI")
        .collect();
    assert_eq!(hits.len(), 1);
    let hit = hits[0];
    assert_eq!((hit.recognition.start, hit.recognition.end), (10, 16));
    assert_eq!(
        hit.cuts.iter().map(|cut| cut.boundary).collect::<Vec<_>>(),
        [11, 3]
    );
    assert_eq!((hit.anchor.start, hit.anchor.end), (11, 12));
}

#[test]
fn nonpalindromic_reverse_hit_uses_reverse_cut_formula() {
    let database = RestrictionDatabase::new(
        "test-orientation-db",
        "1",
        vec![RestrictionEnzyme::new("TEST-AACG", "AACG", 1, 2)],
    )
    .expect("test DB");
    let record = make_record("TTTACGTTTTTT", Topology::Linear, vec![]);
    let analysis = analyze_restriction(&record, &database).expect("reverse analysis");
    assert_eq!(analysis.hits.len(), 1);
    let hit = &analysis.hits[0];
    assert_eq!(hit.recognition.strand, -1);
    assert_eq!(
        hit.cuts
            .iter()
            .map(|cut| (cut.strand, cut.boundary))
            .collect::<Vec<_>>(),
        [(-1, 7), (1, 6)]
    );
    assert_eq!((hit.anchor.start, hit.anchor.end), (7, 8));
}

#[test]
fn genetic_code_provenance_is_versioned() {
    let database = genetic_code_database();
    assert_eq!(database.id, "ncbi-genetic-codes");
    assert_eq!(database.version, "2026-10-30.1");
    assert!(database.sha256.starts_with("sha256:"));
}
