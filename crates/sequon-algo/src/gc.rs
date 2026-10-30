//! Exact GC analysis for normalized IUPAC selections.

use crate::check_normalized;
use crate::error::AnalysisError;
use crate::provenance::{self, AnalysisProvenance};

/// Deterministic GC counts and exact milli-percent rounding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcContent {
    /// Bases guaranteeing G or C: G, C and S.
    pub count: u64,
    /// Total selected bases, including all ambiguity symbols.
    pub total: u64,
    /// Percent multiplied by 1000, rounded with exact half-up rational arithmetic.
    pub percent_milli: u64,
    /// Number of non-ACGT symbols, including S.
    pub ambiguous_count: u64,
    /// Algorithm provenance.
    pub provenance: AnalysisProvenance,
}

/// Computes GC over a non-empty normalized IUPAC sequence.
pub fn gc_content(sequence: &str) -> Result<GcContent, AnalysisError> {
    check_normalized(sequence)?;
    if sequence.is_empty() {
        return Err(AnalysisError::SelectionEmpty);
    }
    let mut count = 0_u64;
    let mut ambiguous_count = 0_u64;
    for base in sequence.chars() {
        if matches!(base, 'G' | 'C' | 'S') {
            count += 1;
        }
        if !matches!(base, 'A' | 'C' | 'G' | 'T') {
            ambiguous_count += 1;
        }
    }
    let total = sequence.len() as u64;
    let percent_milli = (count * 200_000 + total) / (2 * total);
    Ok(GcContent {
        count,
        total,
        percent_milli,
        ambiguous_count,
        provenance: provenance::gc(),
    })
}
