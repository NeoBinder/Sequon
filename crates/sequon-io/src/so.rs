//! Shared static feature-key to Sequence Ontology term table.
//!
//! Both format adapters map a source feature key through this one table, so a
//! GenBank key and the equivalent SnapGene feature type get the same term. The
//! table is the proposal in docs/design/genbank-adapter.md section 7.2, checked
//! against Sequence Ontology OBO release 2026-08-07. A key that is not listed
//! gets so_term = null; adapters never guess.

/// Returns the SO identifier for a feature key, when the static table defines one.
///
/// Matching is case-sensitive because both source formats write feature keys and
/// types with their own casing.
pub(crate) fn so_term(key: &str) -> Option<&'static str> {
    let term = match key {
        "source" => "SO:0002206",
        "gene" => "SO:0000704",
        "CDS" => "SO:0000316",
        "mRNA" => "SO:0000234",
        "tRNA" => "SO:0000253",
        "rRNA" => "SO:0000252",
        "ncRNA" => "SO:0000655",
        "tmRNA" => "SO:0000584",
        "misc_RNA" => "SO:0000673",
        "promoter" => "SO:0000167",
        "terminator" => "SO:0000141",
        "enhancer" => "SO:0000165",
        "silencer" => "SO:0000625",
        "insulator" => "SO:0000627",
        "RBS" => "SO:0000139",
        "polyA_signal" => "SO:0000551",
        "polyA_site" => "SO:0000553",
        "TATA_signal" => "SO:0000174",
        "-35_signal" => "SO:0000176",
        "-10_signal" => "SO:0000175",
        "GC_signal" => "SO:0000173",
        "CAAT_signal" => "SO:0000172",
        "primer_bind" => "SO:0005850",
        "protein_bind" => "SO:0000410",
        "regulatory" => "SO:0005836",
        "rep_origin" => "SO:0000296",
        "oriT" => "SO:0000724",
        "LTR" => "SO:0000286",
        "repeat_region" => "SO:0000657",
        "mobile_element" => "SO:0001037",
        "sig_peptide" => "SO:0000418",
        "transit_peptide" => "SO:0000725",
        "mat_peptide" => "SO:0000419",
        "propeptide" => "SO:0001062",
        "misc_feature" => "SO:0000110",
        "variation" => "SO:0001059",
        "operon" => "SO:0000178",
        "intron" => "SO:0000188",
        "exon" => "SO:0000147",
        "5'UTR" => "SO:0000204",
        "3'UTR" => "SO:0000205",
        "assembly_gap" => "SO:0000730",
        "centromere" => "SO:0000577",
        "telomere" => "SO:0000624",
        _ => return None,
    };
    Some(term)
}
