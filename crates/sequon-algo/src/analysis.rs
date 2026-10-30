//! Record-level and selection-level derived analysis APIs.

use sequon_model::Record;

use crate::error::{validate_record, AnalysisError};
use crate::extract::{extract_selection, Selection};
use crate::gc::{gc_content, GcContent};
use crate::provenance::{self, AnalysisProvenance};
use crate::restrict::{
    analyze_restriction, restriction_database, RestrictionAmbiguity, RestrictionDatabase,
    RestrictionHit,
};
use crate::reverse_complement;
use crate::translate::{
    genetic_code_database, translate_cds_validated, translate_selection, CdsTranslation,
    SelectionTranslationOptions,
};

/// One requested selection operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionOperation {
    /// Forward recorded-strand DNA.
    Dna,
    /// Reverse complement of the selected DNA.
    ReverseComplement,
    /// Exact GC counts and milli-percent.
    Gc,
    /// Translation with explicit strand/frame/table.
    Translation(SelectionTranslationOptions),
}

/// Selection results with only the requested fields populated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionAnalysis {
    /// Exact input selection.
    pub selection: Selection,
    /// Forward selected DNA when requested.
    pub dna: Option<String>,
    /// Reverse complement when requested.
    pub revcomp: Option<String>,
    /// GC result when requested.
    pub gc: Option<GcContent>,
    /// Translation result when requested.
    pub translation: Option<CdsTranslation>,
    /// Extraction plus each requested algorithm's provenance.
    pub provenance: Vec<AnalysisProvenance>,
}

/// Analyzes one positive-length selection with unique, non-empty operations.
pub fn analyze_selection(
    record: &Record,
    selection: Selection,
    operations: &[SelectionOperation],
) -> Result<SelectionAnalysis, AnalysisError> {
    validate_record(record)?;
    if operations.is_empty() {
        return Err(AnalysisError::InvalidSelectionOperation {
            reason: "at least one operation is required",
        });
    }
    for (index, operation) in operations.iter().enumerate() {
        if operations[..index]
            .iter()
            .any(|previous| std::mem::discriminant(previous) == std::mem::discriminant(operation))
        {
            return Err(AnalysisError::InvalidSelectionOperation {
                reason: "operations must not contain duplicates",
            });
        }
    }
    let extracted = extract_selection(record, selection)?;
    let mut result = SelectionAnalysis {
        selection,
        dna: None,
        revcomp: None,
        gc: None,
        translation: None,
        provenance: vec![extracted.provenance.clone()],
    };
    for operation in operations {
        match operation {
            SelectionOperation::Dna => result.dna = Some(extracted.dna.clone()),
            SelectionOperation::ReverseComplement => {
                result.revcomp = Some(reverse_complement(&extracted.dna)?);
                result.provenance.push(provenance::reverse_complement());
            }
            SelectionOperation::Gc => {
                let gc = gc_content(&extracted.dna)?;
                result.provenance.push(gc.provenance.clone());
                result.gc = Some(gc);
            }
            SelectionOperation::Translation(options) => {
                let translation = translate_selection(record, selection, *options)?;
                result.provenance.push(translation.provenance.clone());
                result.translation = Some(translation);
            }
        }
    }
    Ok(result)
}

/// Per-CDS outcome; unsupported CDSs remain explicit in source order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CdsAnalysis {
    /// Successful translation.
    Translation(Box<CdsTranslation>),
    /// Typed per-feature failure.
    Error(CdsAnalysisError),
}

/// One typed CDS failure with stable feature identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CdsAnalysisError {
    /// Canonical source-order feature index.
    pub feature_index: usize,
    /// Stable AnalysisError code.
    pub code: &'static str,
    /// Safe diagnostic without sequence content.
    pub message: String,
}

/// Complete record-level analysis without platform record/version identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordAnalysis {
    /// One result or typed error for every canonical CDS, in source order.
    pub cds: Vec<CdsAnalysis>,
    /// Confirmed restriction hits.
    pub restriction_sites: Vec<RestrictionHit>,
    /// Possible but unconfirmed restriction matches.
    pub restriction_ambiguities: Vec<RestrictionAmbiguity>,
    /// Translation and restriction algorithm/database provenance.
    pub provenance: Vec<AnalysisProvenance>,
}

/// Analyzes every CDS and the built-in restriction database.
pub fn analyze_record(record: &Record) -> Result<RecordAnalysis, AnalysisError> {
    analyze_record_with_database(record, &restriction_database())
}

/// Analyzes every CDS and a supplied restriction database without mutating the Record.
pub fn analyze_record_with_database(
    record: &Record,
    database: &RestrictionDatabase,
) -> Result<RecordAnalysis, AnalysisError> {
    validate_record(record)?;
    let mut cds = Vec::new();
    for (feature_index, feature) in record.features.iter().enumerate() {
        if feature.key != "CDS" {
            continue;
        }
        match translate_cds_validated(record, feature, feature_index) {
            Ok(translation) => cds.push(CdsAnalysis::Translation(Box::new(translation))),
            Err(error) => cds.push(CdsAnalysis::Error(CdsAnalysisError {
                feature_index,
                code: error.code(),
                message: error.to_string(),
            })),
        }
    }
    let restriction = analyze_restriction(record, database)?;
    Ok(RecordAnalysis {
        cds,
        restriction_sites: restriction.hits,
        restriction_ambiguities: restriction.ambiguities,
        provenance: vec![
            provenance::translation(genetic_code_database()),
            restriction.provenance,
        ],
    })
}
