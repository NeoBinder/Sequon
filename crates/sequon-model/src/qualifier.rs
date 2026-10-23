//! Ordered feature qualifier pairs.

use serde::{Deserialize, Serialize};

/// One ordered qualifier key/value pair.
///
/// Qualifiers are an array rather than a map so duplicate keys and input order
/// survive, which GenBank requires for repeated /db_xref and /note values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Qualifier {
    /// Qualifier name without the leading slash; never empty in a valid record.
    pub key: String,
    /// Qualifier value, or null for a bare qualifier such as /pseudo.
    #[serde(default)]
    pub value: Option<String>,
}

impl Qualifier {
    /// Builds a qualifier with a string value.
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Qualifier {
            key: key.into(),
            value: Some(value.into()),
        }
    }

    /// Builds a bare qualifier whose value is null.
    pub fn bare(key: impl Into<String>) -> Self {
        Qualifier {
            key: key.into(),
            value: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_keep_value_shape() {
        assert_eq!(Qualifier::new("gene", "gfp").value.as_deref(), Some("gfp"));
        assert_eq!(Qualifier::bare("pseudo").value, None);
    }
}
