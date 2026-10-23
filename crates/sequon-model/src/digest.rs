//! Digest values carried by a canonical record.
//!
//! This module validates and stores the textual form of a digest. It never
//! hashes, because the model must not depend on a hashing crate: sequon-algo
//! computes and verifies digests. Record::validate() therefore checks that a
//! digest is syntactically canonical, while sequon_algo::verify_digests()
//! checks that it carries the right value.

use std::error::Error;
use std::fmt;
use std::str::FromStr;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

/// Digest algorithm identifier used by version 1 canonical records.
pub const DIGEST_ALGORITHM_V1: &str = "sequon-digest/1";

/// Prefix of every digest string in canonical JSON.
const SHA256_PREFIX: &str = "sha256:";

/// Number of hexadecimal digits after the prefix.
const SHA256_HEX_LEN: usize = 64;

/// Lowercase hexadecimal digits used to format and validate a digest.
const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

/// Error returned when a string is not a canonical digest value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DigestFormatError {
    /// The value does not start with the sha256: prefix.
    MissingPrefix,
    /// The hexadecimal payload does not have exactly 64 characters.
    WrongLength {
        /// Number of characters found after the prefix.
        found: usize,
    },
    /// The payload contains a character that is not a lowercase hex digit.
    InvalidCharacter {
        /// Character index inside the hexadecimal payload.
        index: usize,
        /// The offending character.
        character: char,
    },
}

impl fmt::Display for DigestFormatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DigestFormatError::MissingPrefix => {
                formatter.write_str("digest must start with the prefix 'sha256:'")
            }
            DigestFormatError::WrongLength { found } => write!(
                formatter,
                "digest payload must be exactly {SHA256_HEX_LEN} lowercase hexadecimal characters, found {found}"
            ),
            DigestFormatError::InvalidCharacter { index, character } => write!(
                formatter,
                "digest payload contains '{character}' at index {index}; expected a lowercase hexadecimal digit"
            ),
        }
    }
}

impl Error for DigestFormatError {}

/// A validated digest string: sha256: followed by 64 lowercase hex digits.
///
/// The canonical form is fixed by docs/design/canonical-json-profile.md and
/// schemas/sequon-record-1.schema.json. Values are constructed through
/// parse(), from_sha256_bytes() or TryFrom, so a wrong shape cannot be built by
/// accident.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DigestValue(String);

impl DigestValue {
    /// Parses a value, accepting only the canonical sha256: form with 64 lowercase hex digits.
    pub fn parse(value: &str) -> Result<Self, DigestFormatError> {
        match value.strip_prefix(SHA256_PREFIX) {
            None => Err(DigestFormatError::MissingPrefix),
            Some(payload) => {
                if payload.len() != SHA256_HEX_LEN {
                    return Err(DigestFormatError::WrongLength {
                        found: payload.len(),
                    });
                }
                for (index, character) in payload.char_indices() {
                    if !character.is_ascii_digit() && !('a'..='f').contains(&character) {
                        return Err(DigestFormatError::InvalidCharacter { index, character });
                    }
                }
                Ok(DigestValue(value.to_owned()))
            }
        }
    }

    /// Builds the canonical digest string from a raw 32-byte SHA-256 digest.
    ///
    /// This only formats bytes as lowercase hexadecimal; it does not hash.
    pub fn from_sha256_bytes(digest: [u8; 32]) -> Self {
        let mut value = String::with_capacity(SHA256_PREFIX.len() + SHA256_HEX_LEN);
        value.push_str(SHA256_PREFIX);
        for byte in digest {
            let high = HEX_DIGITS[usize::from(byte >> 4)];
            let low = HEX_DIGITS[usize::from(byte & 0x0f)];
            value.push(char::from(high));
            value.push(char::from(low));
        }
        DigestValue(value)
    }

    /// Returns the digest as sha256: followed by 64 lowercase hex digits.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns whether the stored string still follows the canonical form.
    ///
    /// Values built by this module always do; Record::validate() calls this as
    /// a defensive check and to state the digest rule in one place.
    pub fn is_canonical_form(&self) -> bool {
        DigestValue::parse(&self.0).is_ok()
    }
}

impl fmt::Display for DigestValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for DigestValue {
    type Err = DigestFormatError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        DigestValue::parse(value)
    }
}

impl TryFrom<&str> for DigestValue {
    type Error = DigestFormatError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        DigestValue::parse(value)
    }
}

impl Serialize for DigestValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for DigestValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct DigestVisitor;

        impl Visitor<'_> for DigestVisitor {
            type Value = DigestValue;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(
                    "a digest string of the form 'sha256:' followed by 64 lowercase hexadecimal characters",
                )
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                DigestValue::parse(value).map_err(E::custom)
            }
        }

        deserializer.deserialize_str(DigestVisitor)
    }
}

/// The two digests that identify a record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Digests {
    /// Digest algorithm identifier; DIGEST_ALGORITHM_V1 in version 1.
    pub algorithm: String,
    /// SHA-256 of the normalized sequence in its recorded orientation.
    pub sequence: DigestValue,
    /// SHA-256 of the canonical form: min of the sequence and its reverse
    /// complement for linear records, and the lexicographically smallest
    /// rotation of either orientation for circular records.
    pub canonical: DigestValue,
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEX: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn accepts_canonical_digest() {
        let value = format!("sha256:{HEX}");
        let parsed = DigestValue::parse(&value).expect("canonical digest must parse");
        assert_eq!(parsed.as_str(), value);
        assert!(parsed.is_canonical_form());
    }

    #[test]
    fn rejects_missing_prefix() {
        assert_eq!(
            DigestValue::parse(HEX),
            Err(DigestFormatError::MissingPrefix)
        );
    }

    #[test]
    fn rejects_wrong_length() {
        assert_eq!(
            DigestValue::parse("sha256:abcd"),
            Err(DigestFormatError::WrongLength { found: 4 })
        );
        let long = format!("sha256:{HEX}0");
        assert_eq!(
            DigestValue::parse(&long),
            Err(DigestFormatError::WrongLength { found: 65 })
        );
    }

    #[test]
    fn rejects_uppercase_and_non_hex_characters() {
        let uppercase = format!("sha256:{}", HEX.to_uppercase());
        assert!(matches!(
            DigestValue::parse(&uppercase),
            Err(DigestFormatError::InvalidCharacter { .. })
        ));
        let non_hex = format!("sha256:{}", HEX.replacen('0', "g", 1));
        match DigestValue::parse(&non_hex) {
            Err(DigestFormatError::InvalidCharacter { index, character }) => {
                assert_eq!(index, 0);
                assert_eq!(character, 'g');
            }
            other => panic!("expected an invalid character error, got {other:?}"),
        }
    }

    #[test]
    fn formats_raw_bytes_as_lowercase_hex() {
        let mut bytes = [0u8; 32];
        bytes[0] = 0xab;
        bytes[31] = 0x0f;
        let value = DigestValue::from_sha256_bytes(bytes);
        assert!(value.as_str().starts_with("sha256:ab"));
        assert!(value.as_str().ends_with("0f"));
        assert_eq!(value.as_str().len(), "sha256:".len() + 64);
        assert!(value.is_canonical_form());
    }

    #[test]
    fn digest_format_error_messages_name_the_rule() {
        assert!(DigestFormatError::MissingPrefix
            .to_string()
            .contains("sha256:"));
        assert!(DigestFormatError::WrongLength { found: 3 }
            .to_string()
            .contains("64"));
    }
}
