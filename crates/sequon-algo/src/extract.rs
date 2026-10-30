//! Canonical location and selection extraction with exact base mappings.

use sequon_model::{Location, Record, Topology, OPERATOR_JOIN, OPERATOR_ORDER, OPERATOR_SPAN};

use crate::error::{validate_record, AnalysisError};
use crate::iupac::complement;
use crate::provenance::{self, AnalysisProvenance};
use crate::reverse_complement;

/// One positive-length 0-based half-open selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    /// Inclusive start.
    pub start: u64,
    /// Exclusive end, possibly greater than length for one circular wrap.
    pub end: u64,
}

impl Selection {
    /// Builds a selection; bounds are checked against a Record on extraction.
    pub fn new(start: u64, end: u64) -> Self {
        Selection { start, end }
    }
}

/// One coordinate range in a derived biological-order segment list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoordinateSegment {
    /// Inclusive 0-based start.
    pub start: u64,
    /// Exclusive 0-based end.
    pub end: u64,
}

/// Continuity semantics of an extracted location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Continuity {
    /// One canonical span.
    Contiguous,
    /// A join with no filler bases inserted.
    Joined,
    /// Known order segments with unknown gaps; never a complete-CDS claim.
    OrderedUnknownGaps,
}

/// One known biological-order segment and its exact source coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedSegment {
    /// Resolved source start.
    pub start: u64,
    /// Resolved source end.
    pub end: u64,
    /// Segment bases after whole-location strand orientation.
    pub dna: String,
    /// Original source start-partial marker.
    pub start_partial: bool,
    /// Original source end-partial marker.
    pub end_partial: bool,
}

/// Extracted DNA, known segments and provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocationExtraction {
    /// Complete extracted DNA after whole-location strand orientation.
    pub dna: String,
    /// Known biological-order segments; negative strand reverses their order without sorting.
    pub segments: Vec<ExtractedSegment>,
    /// Continuity semantics.
    pub continuity: Continuity,
    /// Whole-location strand used for extraction.
    pub strand: i8,
    /// Algorithm provenance.
    pub provenance: AnalysisProvenance,
    pub(crate) mapped_bases: Vec<MappedBase>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MappedBase {
    pub(crate) base: char,
    pub(crate) start: u64,
    pub(crate) end: u64,
}

/// Extracts any canonical location after validating the Record and location.
pub fn extract_location(
    record: &Record,
    location: &Location,
) -> Result<LocationExtraction, AnalysisError> {
    validate_record(record)?;
    extract_location_validated(record, location)
}

/// Extracts a positive-length linear or circular selection.
pub fn extract_selection(
    record: &Record,
    selection: Selection,
) -> Result<LocationExtraction, AnalysisError> {
    validate_record(record)?;
    validate_selection(record, selection)?;
    extract_location_validated(record, &Location::span(1, selection.start, selection.end))
}

/// Checks one positive-length selection against the canonical topology-aware
/// bounds without extracting it.
///
/// This is the single implementation of the selection rule shared by extraction
/// and annotation transforms: on a linear record of length L a selection
/// satisfies 0 <= start < end <= L; on a circular record it satisfies
/// 0 <= start < L, end > start and end <= start + L (at most one wrap).
pub(crate) fn validate_selection(
    record: &Record,
    selection: Selection,
) -> Result<(), AnalysisError> {
    if selection.start == selection.end {
        return Err(AnalysisError::SelectionEmpty);
    }
    let valid = match record.topology {
        Topology::Linear => selection.start < selection.end && selection.end <= record.length,
        Topology::Circular => {
            selection.start < record.length
                && selection.end > selection.start
                && selection.end <= selection.start + record.length
        }
    };
    if !valid {
        return Err(AnalysisError::SelectionOutOfRange {
            start: selection.start,
            end: selection.end,
            length: record.length,
            topology: record.topology,
        });
    }
    Ok(())
}

pub(crate) fn extract_location_validated(
    record: &Record,
    location: &Location,
) -> Result<LocationExtraction, AnalysisError> {
    validate_location(location, record)?;
    let continuity = match location.operator.as_str() {
        OPERATOR_SPAN => Continuity::Contiguous,
        OPERATOR_JOIN => Continuity::Joined,
        OPERATOR_ORDER => Continuity::OrderedUnknownGaps,
        _ => {
            return Err(AnalysisError::InvalidLocation {
                reason: "operator must be span, join or order",
            })
        }
    };
    let resolved = location.resolved_segments(record.length);
    let mut forward_segments = Vec::with_capacity(resolved.len());
    let mut mapped_bases = Vec::new();
    for segment in resolved {
        let start = usize::try_from(segment.start).map_err(|_| AnalysisError::InvalidLocation {
            reason: "coordinate does not fit this platform",
        })?;
        let end = usize::try_from(segment.end).map_err(|_| AnalysisError::InvalidLocation {
            reason: "coordinate does not fit this platform",
        })?;
        let dna = record.sequence[start..end].to_owned();
        for (offset, base) in dna.chars().enumerate() {
            let position = segment.start + offset as u64;
            mapped_bases.push(MappedBase {
                base,
                start: position,
                end: position + 1,
            });
        }
        forward_segments.push(ExtractedSegment {
            start: segment.start,
            end: segment.end,
            dna,
            start_partial: segment.start_partial,
            end_partial: segment.end_partial,
        });
    }

    let (segments, strand) = match location.strand {
        1 | 0 => (forward_segments, location.strand),
        -1 => {
            mapped_bases.reverse();
            for mapped in &mut mapped_bases {
                mapped.base = complement(mapped.base).ok_or(AnalysisError::InvalidLocation {
                    reason: "sequence contains a non-IUPAC base",
                })?;
            }
            let mut reversed = Vec::with_capacity(forward_segments.len());
            for segment in forward_segments.into_iter().rev() {
                reversed.push(ExtractedSegment {
                    start: segment.start,
                    end: segment.end,
                    dna: reverse_complement(&segment.dna)?,
                    start_partial: segment.start_partial,
                    end_partial: segment.end_partial,
                });
            }
            (reversed, -1)
        }
        _ => {
            return Err(AnalysisError::InvalidLocation {
                reason: "strand must be -1, 0 or 1",
            })
        }
    };
    let dna = mapped_bases.iter().map(|mapped| mapped.base).collect();
    Ok(LocationExtraction {
        dna,
        segments,
        continuity,
        strand,
        provenance: provenance::extraction(),
        mapped_bases,
    })
}

/// Checks one canonical location against the operator, strand, segment and
/// topology-aware bounds rules without extracting it.
pub(crate) fn validate_location(location: &Location, record: &Record) -> Result<(), AnalysisError> {
    if !matches!(location.strand, -1..=1) {
        return Err(AnalysisError::InvalidLocation {
            reason: "strand must be -1, 0 or 1",
        });
    }
    if location.segments.is_empty() {
        return Err(AnalysisError::InvalidLocation {
            reason: "location requires at least one segment",
        });
    }
    match location.operator.as_str() {
        OPERATOR_SPAN if location.segments.len() != 1 => {
            return Err(AnalysisError::InvalidLocation {
                reason: "span requires exactly one segment",
            });
        }
        OPERATOR_SPAN | OPERATOR_JOIN | OPERATOR_ORDER => {}
        _ => {
            return Err(AnalysisError::InvalidLocation {
                reason: "operator must be span, join or order",
            })
        }
    }
    for (segment_index, segment) in location.segments.iter().enumerate() {
        let valid = match record.topology {
            Topology::Linear => segment.start <= segment.end && segment.end <= record.length,
            Topology::Circular if segment.start == segment.end => segment.start <= record.length,
            Topology::Circular => {
                segment.start < record.length
                    && segment.end > segment.start
                    && segment.end <= segment.start + record.length
            }
        };
        if !valid {
            return Err(AnalysisError::LocationOutOfRange {
                segment_index,
                start: segment.start,
                end: segment.end,
                length: record.length,
                topology: record.topology,
            });
        }
    }
    Ok(())
}
