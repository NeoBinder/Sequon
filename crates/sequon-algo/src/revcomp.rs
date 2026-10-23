//! Reverse complement over the full IUPAC DNA complement map.
//!
//! The map is A-T, C-G, R-Y, S-S, W-W, K-M, B-V, D-H and N-N. Reverse
//! complementing keeps every segment boundary of the input and only changes the
//! order and the identity of the bases, which is why it is the strand
//! equivalence used by the canonical digest.

use crate::normalize::{check_normalized, SequenceError};

/// Returns the reverse complement of a normalized sequence.
///
/// The input must be uppercase IUPAC DNA; a lowercase or otherwise invalid
/// sequence is rejected instead of being complemented silently. Reverse
/// complementing an empty sequence returns an empty string.
pub fn reverse_complement(sequence: &str) -> Result<String, SequenceError> {
    check_normalized(sequence)?;
    let characters: Vec<char> = sequence.chars().collect();
    let mut complemented = String::with_capacity(sequence.len());
    for (offset, character) in characters.iter().rev().enumerate() {
        match complement(*character) {
            Some(mapped) => complemented.push(mapped),
            None => {
                let index = characters.len() - offset - 1;
                return Err(SequenceError::invalid(index, *character));
            }
        }
    }
    Ok(complemented)
}

/// Returns the IUPAC complement of one base, or None for a non-IUPAC character.
fn complement(character: char) -> Option<char> {
    match character {
        'A' => Some('T'),
        'C' => Some('G'),
        'G' => Some('C'),
        'T' => Some('A'),
        'N' => Some('N'),
        'R' => Some('Y'),
        'Y' => Some('R'),
        'S' => Some('S'),
        'W' => Some('W'),
        'K' => Some('M'),
        'M' => Some('K'),
        'B' => Some('V'),
        'V' => Some('B'),
        'D' => Some('H'),
        'H' => Some('D'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complements_every_iupac_base() {
        assert_eq!(reverse_complement("ACGT").expect("valid"), "ACGT");
        assert_eq!(
            reverse_complement("NRYSWKMBDHV").expect("valid"),
            "BDHVKMWSRYN"
        );
        assert_eq!(reverse_complement("A").expect("valid"), "T");
        assert_eq!(reverse_complement("").expect("valid"), "");
    }

    #[test]
    fn revcomp_is_an_involution() {
        for sequence in ["ATGCGTACGTAGCTAGCTAGCATCGATCGA", "NRYSWKMBDHV"] {
            let once = reverse_complement(sequence).expect("valid");
            let twice = reverse_complement(&once).expect("valid");
            assert_eq!(twice, sequence);
        }
    }

    #[test]
    fn rejects_non_normalized_input() {
        let error = reverse_complement("acgt").expect_err("lowercase is not normalized");
        assert_eq!(error.character(), 'a');
        assert!(reverse_complement("ACGU").is_err());
    }

    #[test]
    fn canonical_model_reverse_complement_map() {
        // The pairs named in docs/design/canonical-model.md, one at a time.
        let pairs = [
            ('A', 'T'),
            ('C', 'G'),
            ('R', 'Y'),
            ('S', 'S'),
            ('W', 'W'),
            ('K', 'M'),
            ('B', 'V'),
            ('D', 'H'),
            ('N', 'N'),
        ];
        for (forward, reverse) in pairs {
            let input = forward.to_string();
            assert_eq!(
                reverse_complement(&input).expect("valid"),
                reverse.to_string(),
                "complement of {forward}"
            );
        }
    }
}
