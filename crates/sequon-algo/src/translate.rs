//! CDS and selection translation with exact codon/AA coordinate mappings.

use sequon_model::{Feature, Location, Record, OPERATOR_ORDER};

use crate::error::{validate_record, AnalysisError};
use crate::extract::{
    extract_location_validated, extract_selection, Continuity, CoordinateSegment,
    LocationExtraction, MappedBase, Selection,
};
use crate::iupac::expand_codon;
use crate::provenance::{self, sha256_hex, AnalysisProvenance, DatabaseVersion};

/// Amino-acid or special translation result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AminoAcid {
    /// A / Ala.
    Alanine,
    /// R / Arg.
    Arginine,
    /// N / Asn.
    Asparagine,
    /// D / Asp.
    AsparticAcid,
    /// C / Cys.
    Cysteine,
    /// E / Glu.
    GlutamicAcid,
    /// Q / Gln.
    Glutamine,
    /// G / Gly.
    Glycine,
    /// H / His.
    Histidine,
    /// I / Ile.
    Isoleucine,
    /// L / Leu.
    Leucine,
    /// K / Lys.
    Lysine,
    /// M / Met.
    Methionine,
    /// F / Phe.
    Phenylalanine,
    /// P / Pro.
    Proline,
    /// S / Ser.
    Serine,
    /// T / Thr.
    Threonine,
    /// W / Trp.
    Tryptophan,
    /// Y / Tyr.
    Tyrosine,
    /// V / Val.
    Valine,
    /// * / Ter.
    Stop,
    /// X / Xaa for a non-unanimous ambiguous codon.
    Unknown,
}

impl AminoAcid {
    /// Returns the one-letter symbol.
    pub fn one_letter(self) -> char {
        match self {
            AminoAcid::Alanine => 'A',
            AminoAcid::Arginine => 'R',
            AminoAcid::Asparagine => 'N',
            AminoAcid::AsparticAcid => 'D',
            AminoAcid::Cysteine => 'C',
            AminoAcid::GlutamicAcid => 'E',
            AminoAcid::Glutamine => 'Q',
            AminoAcid::Glycine => 'G',
            AminoAcid::Histidine => 'H',
            AminoAcid::Isoleucine => 'I',
            AminoAcid::Leucine => 'L',
            AminoAcid::Lysine => 'K',
            AminoAcid::Methionine => 'M',
            AminoAcid::Phenylalanine => 'F',
            AminoAcid::Proline => 'P',
            AminoAcid::Serine => 'S',
            AminoAcid::Threonine => 'T',
            AminoAcid::Tryptophan => 'W',
            AminoAcid::Tyrosine => 'Y',
            AminoAcid::Valine => 'V',
            AminoAcid::Stop => '*',
            AminoAcid::Unknown => 'X',
        }
    }

    /// Returns the three-letter symbol.
    pub fn three_letter(self) -> &'static str {
        match self {
            AminoAcid::Alanine => "Ala",
            AminoAcid::Arginine => "Arg",
            AminoAcid::Asparagine => "Asn",
            AminoAcid::AsparticAcid => "Asp",
            AminoAcid::Cysteine => "Cys",
            AminoAcid::GlutamicAcid => "Glu",
            AminoAcid::Glutamine => "Gln",
            AminoAcid::Glycine => "Gly",
            AminoAcid::Histidine => "His",
            AminoAcid::Isoleucine => "Ile",
            AminoAcid::Leucine => "Leu",
            AminoAcid::Lysine => "Lys",
            AminoAcid::Methionine => "Met",
            AminoAcid::Phenylalanine => "Phe",
            AminoAcid::Proline => "Pro",
            AminoAcid::Serine => "Ser",
            AminoAcid::Threonine => "Thr",
            AminoAcid::Tryptophan => "Trp",
            AminoAcid::Tyrosine => "Tyr",
            AminoAcid::Valine => "Val",
            AminoAcid::Stop => "Ter",
            AminoAcid::Unknown => "Xaa",
        }
    }
}

/// One translated codon and its exact biological-order genomic pieces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodonTranslation {
    /// Zero-based AA index in biological translation order.
    pub aa_index: u64,
    /// Concrete or IUPAC codon after strand orientation.
    pub codon: String,
    /// One-letter result.
    pub aa: char,
    /// Three-letter result.
    pub aa3: String,
    /// One or more exact 0-based half-open pieces in biological 5'->3' order.
    pub segments: Vec<CoordinateSegment>,
}

/// Translation of a CDS or a selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CdsTranslation {
    /// Canonical feature index, or None for a feature-independent selection.
    pub feature_index: Option<usize>,
    /// Extraction continuity.
    pub continuity: Continuity,
    /// Strand used for translation.
    pub strand: i8,
    /// 1-based codon start; selection frame + 1.
    pub codon_start: u8,
    /// NCBI translation table, 1 or 11.
    pub transl_table: u32,
    /// Extracted bases before codon_start/frame offset.
    pub source_length: u64,
    /// Bases consumed by complete codons.
    pub translated_length: u64,
    /// Number of translated codons and AA entries.
    pub translated_codon_count: u64,
    /// One-letter amino-acid string.
    pub aa: String,
    /// Aligned three-letter amino-acid array.
    pub aa3: Vec<String>,
    /// Exact codon mappings in biological order.
    pub codons: Vec<CodonTranslation>,
    /// Input codons containing a non-ACGT symbol.
    pub ambiguous_codon_count: u64,
    /// Ambiguous codons with both stop and non-stop possibilities.
    pub ambiguous_stop_codon_count: u64,
    /// Unanimous stop codons.
    pub stop_codon_count: u64,
    /// Trailing untranslated bases after complete codons, 0..=2.
    pub partial_tail_length: u8,
    /// Trailing bases, never padded or converted to an AA.
    pub partial_tail_dna: String,
    /// First canonical segment's start-partial marker.
    pub start_partial: bool,
    /// Last canonical segment's end-partial marker.
    pub end_partial: bool,
    /// Algorithm and genetic-code database provenance.
    pub provenance: AnalysisProvenance,
}

/// Explicit translation parameters for a feature-independent selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionTranslationOptions {
    /// 1 for forward or -1 for reverse-complement selection translation.
    pub strand: i8,
    /// Number of bases to skip after strand orientation, 0..=2.
    pub frame: u8,
    /// NCBI table 1 or 11.
    pub transl_table: u32,
}

/// Returns the pinned NCBI genetic-code database identity for tables 1 and 11.
pub fn genetic_code_database() -> DatabaseVersion {
    let mut canonical = String::from(
        "ncbi-genetic-codes/2026-10-30.1
tables=1,11
",
    );
    for first in ['T', 'C', 'A', 'G'] {
        for second in ['T', 'C', 'A', 'G'] {
            for third in ['T', 'C', 'A', 'G'] {
                let codon = [first as u8, second as u8, third as u8];
                let aa = standard_codon_aa(codon);
                canonical.push_str(&format!(
                    "{}{}{}={}
",
                    first,
                    second,
                    third,
                    aa.one_letter()
                ));
            }
        }
    }
    DatabaseVersion {
        id: "ncbi-genetic-codes".to_owned(),
        version: "2026-10-30.1".to_owned(),
        sha256: sha256_hex(canonical.as_bytes()),
    }
}

/// Translates one canonical CDS feature by source-order feature index.
pub fn translate_cds(
    record: &Record,
    feature_index: usize,
) -> Result<CdsTranslation, AnalysisError> {
    validate_record(record)?;
    let feature = record
        .features
        .get(feature_index)
        .ok_or(AnalysisError::InvalidLocation {
            reason: "CDS feature index is out of range",
        })?;
    translate_cds_validated(record, feature, feature_index)
}

pub(crate) fn translate_cds_validated(
    record: &Record,
    feature: &Feature,
    feature_index: usize,
) -> Result<CdsTranslation, AnalysisError> {
    if feature.key != "CDS" {
        return Err(AnalysisError::InvalidLocation {
            reason: "feature key is not CDS",
        });
    }
    let codon_start = parse_codon_start(feature)?;
    let transl_table = parse_translation_table(feature)?;
    translate_location_validated(
        record,
        &feature.location,
        codon_start,
        transl_table,
        Some(feature_index),
    )
}

/// Translates a canonical location using explicit codon_start and table values.
pub fn translate_location(
    record: &Record,
    location: &Location,
    codon_start: u8,
    transl_table: u32,
) -> Result<CdsTranslation, AnalysisError> {
    validate_record(record)?;
    translate_location_validated(record, location, codon_start, transl_table, None)
}

fn translate_location_validated(
    record: &Record,
    location: &Location,
    codon_start: u8,
    transl_table: u32,
    feature_index: Option<usize>,
) -> Result<CdsTranslation, AnalysisError> {
    if location.operator == OPERATOR_ORDER {
        return Err(AnalysisError::OrderCdsUnsupported);
    }
    if location.strand == 0 {
        return Err(AnalysisError::UnstrandedTranslation);
    }
    ensure_translation_table(transl_table)?;
    if !(1..=3).contains(&codon_start) {
        return Err(AnalysisError::CodonStartInvalid {
            reason: "value must be 1, 2 or 3",
        });
    }
    let start_partial = location
        .segments
        .first()
        .is_some_and(|segment| segment.start_partial);
    let end_partial = location
        .segments
        .last()
        .is_some_and(|segment| segment.end_partial);
    let extraction = extract_location_validated(record, location)?;
    translate_extracted(
        extraction,
        codon_start,
        transl_table,
        feature_index,
        start_partial,
        end_partial,
    )
}

/// Translates a selection using explicit strand, frame and table.
pub fn translate_selection(
    record: &Record,
    selection: Selection,
    options: SelectionTranslationOptions,
) -> Result<CdsTranslation, AnalysisError> {
    validate_record(record)?;
    if !matches!(options.strand, -1 | 1) {
        return Err(AnalysisError::UnstrandedTranslation);
    }
    ensure_translation_table(options.transl_table)?;
    if options.frame > 2 {
        return Err(AnalysisError::FrameOutOfRange {
            frame: options.frame,
            length: selection.end.saturating_sub(selection.start),
        });
    }
    let forward = extract_selection(record, selection)?;
    let extraction = if options.strand == 1 {
        forward
    } else {
        extract_location_validated(record, &Location::span(-1, selection.start, selection.end))?
    };
    if options.frame as usize > extraction.dna.len() {
        return Err(AnalysisError::FrameOutOfRange {
            frame: options.frame,
            length: extraction.dna.len() as u64,
        });
    }
    translate_extracted(
        extraction,
        options.frame + 1,
        options.transl_table,
        None,
        false,
        false,
    )
}

fn translate_extracted(
    extraction: LocationExtraction,
    codon_start: u8,
    transl_table: u32,
    feature_index: Option<usize>,
    start_partial: bool,
    end_partial: bool,
) -> Result<CdsTranslation, AnalysisError> {
    let offset = usize::from(codon_start - 1);
    if offset > extraction.dna.len() {
        return Err(AnalysisError::CodonStartInvalid {
            reason: "offset exceeds extracted CDS length",
        });
    }
    let remaining = extraction.dna.len() - offset;
    let partial_tail_length = remaining % 3;
    let translated_length = remaining - partial_tail_length;
    let mut codons = Vec::with_capacity(translated_length / 3);
    let mut ambiguous_codon_count = 0_u64;
    let mut ambiguous_stop_codon_count = 0_u64;
    let mut stop_codon_count = 0_u64;

    for (aa_index, mapped) in extraction.mapped_bases[offset..offset + translated_length]
        .chunks(3)
        .enumerate()
    {
        let codon: String = mapped.iter().map(|base| base.base).collect();
        let (aa, has_ambiguity, mixed_stop) = translate_iupac_codon(&codon)?;
        if has_ambiguity {
            ambiguous_codon_count += 1;
        }
        if mixed_stop {
            ambiguous_stop_codon_count += 1;
        }
        if aa == AminoAcid::Stop {
            stop_codon_count += 1;
        }
        codons.push(CodonTranslation {
            aa_index: aa_index as u64,
            codon,
            aa: aa.one_letter(),
            aa3: aa.three_letter().to_owned(),
            segments: biological_segments(mapped, extraction.strand),
        });
    }
    let aa = codons.iter().map(|codon| codon.aa).collect();
    let aa3 = codons.iter().map(|codon| codon.aa3.clone()).collect();
    let partial_tail_dna = extraction.mapped_bases[offset + translated_length..]
        .iter()
        .map(|base| base.base)
        .collect();
    Ok(CdsTranslation {
        feature_index,
        continuity: extraction.continuity,
        strand: extraction.strand,
        codon_start,
        transl_table,
        source_length: extraction.dna.len() as u64,
        translated_length: translated_length as u64,
        translated_codon_count: codons.len() as u64,
        aa,
        aa3,
        codons,
        ambiguous_codon_count,
        ambiguous_stop_codon_count,
        stop_codon_count,
        partial_tail_length: partial_tail_length as u8,
        partial_tail_dna,
        start_partial,
        end_partial,
        provenance: provenance::translation(genetic_code_database()),
    })
}

fn biological_segments(mapped: &[MappedBase], strand: i8) -> Vec<CoordinateSegment> {
    let mut segments = Vec::new();
    let mut first = mapped[0];
    let mut last = mapped[0];
    for base in &mapped[1..] {
        let contiguous = if strand == -1 {
            base.end == last.start
        } else {
            base.start == last.end
        };
        if contiguous {
            last = *base;
        } else {
            segments.push(CoordinateSegment {
                start: first.start.min(last.start),
                end: first.end.max(last.end),
            });
            first = *base;
            last = *base;
        }
    }
    segments.push(CoordinateSegment {
        start: first.start.min(last.start),
        end: first.end.max(last.end),
    });
    segments
}

fn translate_iupac_codon(codon: &str) -> Result<(AminoAcid, bool, bool), AnalysisError> {
    let expanded = expand_codon(codon).ok_or(AnalysisError::InvalidLocation {
        reason: "translation encountered a non-IUPAC codon",
    })?;
    let has_ambiguity = codon
        .bytes()
        .any(|base| !matches!(base, b'A' | b'C' | b'G' | b'T'));
    let mut unanimous: Option<AminoAcid> = None;
    let mut has_stop = false;
    let mut has_non_stop = false;
    for concrete in expanded {
        let aa = standard_codon_aa(concrete);
        has_stop |= aa == AminoAcid::Stop;
        has_non_stop |= aa != AminoAcid::Stop;
        if let Some(current) = unanimous {
            if current != aa {
                unanimous = Some(AminoAcid::Unknown);
            }
        } else {
            unanimous = Some(aa);
        }
    }
    let mixed_stop = has_stop && has_non_stop;
    let aa = if mixed_stop {
        AminoAcid::Unknown
    } else {
        unanimous.unwrap_or(AminoAcid::Unknown)
    };
    Ok((aa, has_ambiguity, mixed_stop))
}

fn parse_codon_start(feature: &Feature) -> Result<u8, AnalysisError> {
    let values: Vec<&str> = feature
        .qualifiers
        .iter()
        .filter(|qualifier| qualifier.key == "codon_start")
        .map(|qualifier| qualifier.value.as_deref().unwrap_or(""))
        .collect();
    if values.is_empty() {
        return Ok(1);
    }
    if values.len() != 1 {
        return Err(AnalysisError::CodonStartInvalid {
            reason: "qualifier must occur at most once",
        });
    }
    let value = values[0]
        .parse::<u8>()
        .map_err(|_| AnalysisError::CodonStartInvalid {
            reason: "value must be the integer 1, 2 or 3",
        })?;
    if !(1..=3).contains(&value) {
        return Err(AnalysisError::CodonStartInvalid {
            reason: "value must be 1, 2 or 3",
        });
    }
    Ok(value)
}

fn parse_translation_table(feature: &Feature) -> Result<u32, AnalysisError> {
    let values: Vec<&str> = feature
        .qualifiers
        .iter()
        .filter(|qualifier| qualifier.key == "transl_table")
        .map(|qualifier| qualifier.value.as_deref().unwrap_or(""))
        .collect();
    if values.is_empty() {
        return Ok(1);
    }
    if values.len() != 1 {
        return Err(AnalysisError::TranslationTableInvalid {
            reason: "qualifier must occur at most once",
        });
    }
    let table = values[0]
        .parse::<u32>()
        .map_err(|_| AnalysisError::TranslationTableInvalid {
            reason: "value must be an integer",
        })?;
    ensure_translation_table(table)?;
    Ok(table)
}

fn ensure_translation_table(table: u32) -> Result<(), AnalysisError> {
    if matches!(table, 1 | 11) {
        Ok(())
    } else {
        Err(AnalysisError::TranslationTableUnsupported { table })
    }
}

fn standard_codon_aa(codon: [u8; 3]) -> AminoAcid {
    use AminoAcid::*;
    match &codon {
        b"TTT" | b"TTC" => Phenylalanine,
        b"TTA" | b"TTG" | b"CTT" | b"CTC" | b"CTA" | b"CTG" => Leucine,
        b"ATT" | b"ATC" | b"ATA" => Isoleucine,
        b"ATG" => Methionine,
        b"GTT" | b"GTC" | b"GTA" | b"GTG" => Valine,
        b"TCT" | b"TCC" | b"TCA" | b"TCG" | b"AGT" | b"AGC" => Serine,
        b"CCT" | b"CCC" | b"CCA" | b"CCG" => Proline,
        b"ACT" | b"ACC" | b"ACA" | b"ACG" => Threonine,
        b"GCT" | b"GCC" | b"GCA" | b"GCG" => Alanine,
        b"TAT" | b"TAC" => Tyrosine,
        b"TAA" | b"TAG" | b"TGA" => Stop,
        b"CAT" | b"CAC" => Histidine,
        b"CAA" | b"CAG" => Glutamine,
        b"AAT" | b"AAC" => Asparagine,
        b"AAA" | b"AAG" => Lysine,
        b"GAT" | b"GAC" => AsparticAcid,
        b"GAA" | b"GAG" => GlutamicAcid,
        b"TGT" | b"TGC" => Cysteine,
        b"TGG" => Tryptophan,
        b"CGT" | b"CGC" | b"CGA" | b"CGG" | b"AGA" | b"AGG" => Arginine,
        b"GGT" | b"GGC" | b"GGA" | b"GGG" => Glycine,
        _ => Unknown,
    }
}
