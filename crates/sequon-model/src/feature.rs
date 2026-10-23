//! Feature annotations.

use serde::{Deserialize, Serialize};

use crate::location::Location;
use crate::qualifier::Qualifier;

/// One feature annotation on a record.
///
/// The field order is the canonical JSON field order: key, so_term, name,
/// location, qualifiers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Feature {
    /// Feature key exactly as the source wrote it, never rewritten and never
    /// empty in a valid record.
    pub key: String,
    /// Optional Sequence Ontology term derived by the adapter from a static
    /// key table. Always emitted; null means not derived, never a guess.
    #[serde(default)]
    pub so_term: Option<String>,
    /// Optional display name from the source. Always emitted; null means the
    /// source provided none.
    #[serde(default)]
    pub name: Option<String>,
    /// Location of the feature on the sequence.
    pub location: Location,
    /// Ordered qualifier pairs; duplicate keys and source order are preserved.
    pub qualifiers: Vec<Qualifier>,
}
