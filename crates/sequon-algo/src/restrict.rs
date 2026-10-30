//! Versioned restriction-site analysis over linear and circular canonical DNA.

use sequon_model::{Record, Topology};

use crate::error::{validate_record, AnalysisError};
use crate::iupac::mask;
use crate::provenance::{self, sha256_hex, AnalysisProvenance, DatabaseVersion};
use crate::reverse_complement;

/// Overhang orientation produced by a restriction enzyme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverhangKind {
    /// 5' overhang.
    FivePrime,
    /// 3' overhang.
    ThreePrime,
    /// Blunt cut.
    Blunt,
}

/// Restriction overhang description.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overhang {
    /// Overhang orientation.
    pub kind: OverhangKind,
    /// Number of bases; zero for blunt.
    pub length: u64,
}

/// One versioned restriction-enzyme definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestrictionEnzyme {
    /// Stable enzyme identifier.
    pub id: String,
    /// 5'->3' IUPAC recognition pattern.
    pub recognition: String,
    /// Top-strand cut offset in recognition 5'->3' orientation.
    pub top_cut: u64,
    /// Bottom-strand cut offset in its own 5'->3' orientation.
    pub bottom_cut: u64,
    /// Derived overhang description.
    pub overhang: Overhang,
}

impl RestrictionEnzyme {
    /// Builds an enzyme; database construction validates offsets and symbols.
    pub fn new(id: &str, recognition: &str, top_cut: u64, bottom_cut: u64) -> Self {
        let pattern_length = recognition.len() as i64;
        let top = top_cut as i64;
        let bottom_forward = pattern_length - bottom_cut as i64;
        let difference = bottom_forward - top;
        let (kind, length) = if difference > 0 {
            (OverhangKind::FivePrime, difference as u64)
        } else if difference < 0 {
            (OverhangKind::ThreePrime, (-difference) as u64)
        } else {
            (OverhangKind::Blunt, 0)
        };
        RestrictionEnzyme {
            id: id.to_owned(),
            recognition: recognition.to_owned(),
            top_cut,
            bottom_cut,
            overhang: Overhang { kind, length },
        }
    }
}

/// A deterministic, versioned restriction database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestrictionDatabase {
    /// Stable database identifier.
    pub id: String,
    /// Release identifier.
    pub version: String,
    /// SHA-256 of the deterministic sorted database bytes.
    pub sha256: String,
    /// Enzyme definitions sorted by stable id.
    pub enzymes: Vec<RestrictionEnzyme>,
}

impl RestrictionDatabase {
    /// Validates and builds a deterministic database.
    pub fn new(
        id: &str,
        version: &str,
        mut enzymes: Vec<RestrictionEnzyme>,
    ) -> Result<Self, AnalysisError> {
        if id.is_empty() || version.is_empty() || enzymes.is_empty() {
            return Err(AnalysisError::InvalidRestrictionDatabase {
                reason: "database id, version and at least one enzyme are required",
            });
        }
        enzymes.sort_by(|left, right| left.id.cmp(&right.id));
        for pair in enzymes.windows(2) {
            if pair[0].id == pair[1].id {
                return Err(AnalysisError::InvalidRestrictionDatabase {
                    reason: "enzyme ids must be unique",
                });
            }
        }
        let mut canonical = format!(
            "{id}/{version}
"
        );
        for enzyme in &enzymes {
            if enzyme.id.is_empty()
                || enzyme.recognition.is_empty()
                || enzyme.top_cut > enzyme.recognition.len() as u64
                || enzyme.bottom_cut > enzyme.recognition.len() as u64
                || !enzyme.recognition.chars().all(|base| mask(base).is_some())
            {
                return Err(AnalysisError::InvalidRestrictionDatabase {
                    reason: "enzyme id, IUPAC recognition and in-range cut offsets are required",
                });
            }
            canonical.push_str(&format!(
                "{}|{}|{}|{}|{:?}|{}
",
                enzyme.id,
                enzyme.recognition,
                enzyme.top_cut,
                enzyme.bottom_cut,
                enzyme.overhang.kind,
                enzyme.overhang.length
            ));
        }
        Ok(RestrictionDatabase {
            id: id.to_owned(),
            version: version.to_owned(),
            sha256: sha256_hex(canonical.as_bytes()),
            enzymes,
        })
    }

    /// Returns the provenance identity of this database.
    pub fn version_identity(&self) -> DatabaseVersion {
        DatabaseVersion {
            id: self.id.clone(),
            version: self.version.clone(),
            sha256: self.sha256.clone(),
        }
    }
}

/// Returns the built-in R11 restriction database.
pub fn restriction_database() -> RestrictionDatabase {
    let enzymes = vec![
        RestrictionEnzyme::new("BamHI", "GGATCC", 1, 1),
        RestrictionEnzyme::new("EcoRI", "GAATTC", 1, 1),
        RestrictionEnzyme::new("HindIII", "AAGCTT", 1, 1),
        RestrictionEnzyme::new("NdeI", "CATATG", 2, 2),
        RestrictionEnzyme::new("NotI", "GCGGCCGC", 2, 2),
        RestrictionEnzyme::new("XhoI", "CTCGAG", 1, 1),
    ];
    match RestrictionDatabase::new("sequon-restriction-enzymes", "2026-10-30.1", enzymes) {
        Ok(database) => database,
        Err(_) => panic!("the built-in restriction database is statically valid"),
    }
}

/// Recognition range and orientation of one match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecognitionRange {
    /// Unwrapped 0-based start.
    pub start: u64,
    /// Unwrapped exclusive end.
    pub end: u64,
    /// 1 forward, -1 reverse, 0 palindromic.
    pub strand: i8,
}

/// One between-base restriction cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestrictionCut {
    /// Strand whose 5'->3' offset produced this cut.
    pub strand: i8,
    /// Between-base boundary, normalized to 0..length for circular records.
    pub boundary: u64,
}

/// Side of a cut used for the one-base UI navigation anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorSide {
    /// Base immediately right of the cut.
    Right,
    /// Base immediately left of a linear terminal cut.
    Left,
}

/// One-base navigation anchor, distinct from the cut boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavigationAnchor {
    /// Anchor base start.
    pub start: u64,
    /// Exclusive anchor end.
    pub end: u64,
    /// Which side of the cut supplied the anchor.
    pub side: AnchorSide,
}

/// One confirmed restriction hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestrictionHit {
    /// Stable enzyme id.
    pub enzyme_id: String,
    /// Recognition coordinates and orientation.
    pub recognition: RecognitionRange,
    /// Top/bottom cut positions in deterministic order.
    pub cuts: Vec<RestrictionCut>,
    /// Overhang produced by the enzyme.
    pub overhang: Overhang,
    /// Separate one-base navigation anchor.
    pub anchor: NavigationAnchor,
}

/// Classification of an unconfirmed possible restriction match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestrictionAmbiguityCode {
    /// Candidate and pattern intersect but candidate is not a subset at every base.
    AmbiguousTemplate,
}

/// One possible but unconfirmed restriction match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestrictionAmbiguity {
    /// Stable enzyme id.
    pub enzyme_id: String,
    /// Recognition coordinates and orientation.
    pub recognition: RecognitionRange,
    /// Always AMBIGUOUS_TEMPLATE semantics.
    pub code: RestrictionAmbiguityCode,
}

/// Restriction hits, explicit ambiguities and provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestrictionAnalysis {
    /// Confirmed hits only.
    pub hits: Vec<RestrictionHit>,
    /// Possible but unconfirmed matches, never counted as cuts.
    pub ambiguities: Vec<RestrictionAmbiguity>,
    /// Algorithm and enzyme database provenance.
    pub provenance: AnalysisProvenance,
}

/// Analyzes a canonical Record with a supplied restriction database.
pub fn analyze_restriction(
    record: &Record,
    database: &RestrictionDatabase,
) -> Result<RestrictionAnalysis, AnalysisError> {
    validate_record(record)?;
    let sequence: Vec<char> = record.sequence.chars().collect();
    let mut hits = Vec::new();
    let mut ambiguities = Vec::new();
    for enzyme in &database.enzymes {
        let forward = &enzyme.recognition;
        let reverse =
            reverse_complement(forward).map_err(|_| AnalysisError::InvalidRestrictionDatabase {
                reason: "recognition pattern is not normalized IUPAC DNA",
            })?;
        let palindromic = forward == &reverse;
        scan_orientation(
            record,
            &sequence,
            enzyme,
            forward,
            1,
            palindromic,
            ScanBuffers {
                hits: &mut hits,
                ambiguities: &mut ambiguities,
            },
        )?;
        if !palindromic {
            scan_orientation(
                record,
                &sequence,
                enzyme,
                &reverse,
                -1,
                false,
                ScanBuffers {
                    hits: &mut hits,
                    ambiguities: &mut ambiguities,
                },
            )?;
        }
    }
    hits.sort_by(|left, right| {
        left.recognition
            .start
            .cmp(&right.recognition.start)
            .then(left.recognition.end.cmp(&right.recognition.end))
            .then(left.enzyme_id.cmp(&right.enzyme_id))
            .then(left.recognition.strand.cmp(&right.recognition.strand))
    });
    ambiguities.sort_by(|left, right| {
        left.recognition
            .start
            .cmp(&right.recognition.start)
            .then(left.recognition.end.cmp(&right.recognition.end))
            .then(left.enzyme_id.cmp(&right.enzyme_id))
            .then(left.recognition.strand.cmp(&right.recognition.strand))
    });
    Ok(RestrictionAnalysis {
        hits,
        ambiguities,
        provenance: provenance::restriction(database.version_identity()),
    })
}

struct ScanBuffers<'a> {
    hits: &'a mut Vec<RestrictionHit>,
    ambiguities: &'a mut Vec<RestrictionAmbiguity>,
}

fn scan_orientation(
    record: &Record,
    sequence: &[char],
    enzyme: &RestrictionEnzyme,
    pattern: &str,
    orientation: i8,
    palindromic: bool,
    buffers: ScanBuffers<'_>,
) -> Result<(), AnalysisError> {
    let pattern: Vec<char> = pattern.chars().collect();
    let length = sequence.len();
    let pattern_length = pattern.len();
    if pattern_length == 0 || pattern_length > length {
        return Ok(());
    }
    let starts: Vec<usize> = match record.topology {
        Topology::Linear => (0..=length - pattern_length).collect(),
        Topology::Circular => (0..length).collect(),
    };
    for start in starts {
        let mut confirmed = true;
        let mut possible = true;
        for (offset, expected) in pattern.iter().enumerate() {
            let position = match record.topology {
                Topology::Linear => start + offset,
                Topology::Circular => (start + offset) % length,
            };
            let candidate = mask(sequence[position]).ok_or(AnalysisError::InvalidLocation {
                reason: "record contains a non-IUPAC base",
            })?;
            let accepted = mask(*expected).ok_or(AnalysisError::InvalidRestrictionDatabase {
                reason: "recognition pattern contains a non-IUPAC base",
            })?;
            if candidate & !accepted != 0 {
                confirmed = false;
            }
            if candidate & accepted == 0 {
                possible = false;
            }
        }
        let recognition = RecognitionRange {
            start: start as u64,
            end: (start + pattern_length) as u64,
            strand: if palindromic { 0 } else { orientation },
        };
        if confirmed {
            let cuts = make_cuts(record, enzyme, start as u64, orientation)?;
            let anchor = navigation_anchor(record, cuts[0].boundary);
            buffers.hits.push(RestrictionHit {
                enzyme_id: enzyme.id.clone(),
                recognition,
                cuts,
                overhang: enzyme.overhang,
                anchor,
            });
        } else if possible {
            buffers.ambiguities.push(RestrictionAmbiguity {
                enzyme_id: enzyme.id.clone(),
                recognition,
                code: RestrictionAmbiguityCode::AmbiguousTemplate,
            });
        }
    }
    Ok(())
}

fn make_cuts(
    record: &Record,
    enzyme: &RestrictionEnzyme,
    start: u64,
    orientation: i8,
) -> Result<Vec<RestrictionCut>, AnalysisError> {
    let pattern_length = enzyme.recognition.len() as u64;
    let raw = if orientation == 1 {
        vec![
            (1_i8, start + enzyme.top_cut),
            (-1_i8, start + (pattern_length - enzyme.bottom_cut)),
        ]
    } else {
        vec![
            (-1_i8, start + (pattern_length - enzyme.top_cut)),
            (1_i8, start + enzyme.bottom_cut),
        ]
    };
    raw.into_iter()
        .map(|(strand, boundary)| {
            let boundary = match record.topology {
                Topology::Linear if boundary <= record.length => boundary,
                Topology::Circular => boundary % record.length,
                Topology::Linear => {
                    return Err(AnalysisError::InvalidRestrictionDatabase {
                        reason: "cut offset falls outside a linear recognition hit",
                    });
                }
            };
            Ok(RestrictionCut { strand, boundary })
        })
        .collect()
}

fn navigation_anchor(record: &Record, boundary: u64) -> NavigationAnchor {
    if record.topology == Topology::Linear && boundary == record.length {
        return NavigationAnchor {
            start: record.length - 1,
            end: record.length,
            side: AnchorSide::Left,
        };
    }
    NavigationAnchor {
        start: boundary,
        end: boundary + 1,
        side: AnchorSide::Right,
    }
}
