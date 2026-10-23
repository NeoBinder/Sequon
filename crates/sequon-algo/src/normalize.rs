//! Sequence normalization.
//!
//! Normalization happens once, in the adapter. The canonical record stores only
//! the normalized form: uppercase, no whitespace and no digits, IUPAC DNA
//! characters only. Digits and whitespace are structural formatting of a GenBank
//! ORIGIN block, not sequence content, so they are removed; every other
//! character outside the IUPAC alphabet is an error rather than a silent
//! replacement.

use std::error::Error;
use std::fmt;

/// Uppercase IUPAC DNA alphabet accepted by the canonical sequence contract.
pub const IUPAC_DNA: &str = "ACGTNRYSWKMBDHV";

/// Error naming the first character that is not an accepted base.
///
/// The error carries the character index and the offending character so an
/// adapter can report the source line. It never carries the surrounding
/// sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceError {
    index: usize,
    character: char,
}

impl SequenceError {
    pub(crate) fn invalid(index: usize, character: char) -> Self {
        SequenceError { index, character }
    }

    /// Returns the character index of the offending character.
    ///
    /// For a normalized-sequence check the index counts characters in the
    /// input. For normalize_sequence() the index counts characters in the raw
    /// input, including the whitespace and digits that were skipped.
    pub fn index(&self) -> usize {
        self.index
    }

    /// Returns the offending character.
    pub fn character(&self) -> char {
        self.character
    }
}

impl fmt::Display for SequenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid sequence character '{character}' at character index {index}; expected uppercase IUPAC DNA ({IUPAC_DNA})",
            character = self.character,
            index = self.index,
        )
    }
}

impl Error for SequenceError {}

/// Normalizes raw sequence text.
///
/// Lowercase bases are uppercased, and ASCII whitespace and digits are removed
/// because they are ORIGIN formatting rather than content. Every other
/// character outside the IUPAC DNA alphabet is rejected with a typed error;
/// nothing is substituted or dropped silently. An input that normalizes to the
/// empty string is not rejected here: an empty sequence is a record-level error
/// (Record::validate() and the adapter's own typed diagnostic).
pub fn normalize_sequence(input: &str) -> Result<String, SequenceError> {
    let mut normalized = String::with_capacity(input.len());
    for (index, character) in input.chars().enumerate() {
        if character.is_ascii_whitespace() || character.is_ascii_digit() {
            continue;
        }
        let uppercase = character.to_ascii_uppercase();
        if !IUPAC_DNA.contains(uppercase) {
            return Err(SequenceError::invalid(index, character));
        }
        normalized.push(uppercase);
    }
    Ok(normalized)
}

/// Checks that text is already normalized: uppercase IUPAC DNA only.
///
/// Use this before computing a digest over text whose provenance is not already
/// known to be normalized.
pub fn check_normalized(sequence: &str) -> Result<(), SequenceError> {
    for (index, character) in sequence.chars().enumerate() {
        if !IUPAC_DNA.contains(character) {
            return Err(SequenceError::invalid(index, character));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uppercases_and_removes_formatting() {
        let normalized = normalize_sequence("atgc gtcg\ntta 1234 cgtg").expect("valid input");
        assert_eq!(normalized, "ATGCGTCGTTACGTG");
    }

    #[test]
    fn accepts_iupac_ambiguity_codes() {
        let normalized = normalize_sequence("nryswkmbdhv").expect("valid input");
        assert_eq!(normalized, "NRYSWKMBDHV");
    }

    #[test]
    fn rejects_characters_outside_the_alphabet() {
        let error = normalize_sequence("ACGTU").expect_err("U is not a DNA base");
        assert_eq!(error.character(), 'U');
        assert_eq!(error.index(), 4);

        let error = normalize_sequence("AC-GT").expect_err("dash is not a base");
        assert_eq!(error.character(), '-');
        assert_eq!(error.index(), 2);

        let error = normalize_sequence("ACG\u{e9}").expect_err("non-ASCII is not a base");
        assert_eq!(error.character(), '\u{e9}');
        assert_eq!(error.index(), 3);
    }

    #[test]
    fn empty_input_normalizes_to_empty_output() {
        assert_eq!(normalize_sequence("").expect("empty input"), "");
        assert_eq!(
            normalize_sequence("  \n\t123").expect("formatting only"),
            ""
        );
    }

    #[test]
    fn check_normalized_accepts_canonical_text_and_rejects_lowercase() {
        assert!(check_normalized("ACGTNRYSWKMBDHV").is_ok());
        let error = check_normalized("acgt").expect_err("lowercase is not normalized");
        assert_eq!(error.character(), 'a');
        assert_eq!(error.index(), 0);
        assert!(check_normalized("").is_ok());
    }

    #[test]
    fn error_message_names_the_character_and_the_alphabet() {
        let error = normalize_sequence("ACGX").expect_err("X is not a base");
        let message = error.to_string();
        assert!(message.contains('X'));
        assert!(message.contains("ACGTNRYSWKMBDHV"));
    }
}
