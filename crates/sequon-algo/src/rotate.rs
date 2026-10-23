//! Minimal rotation (Booth's algorithm).
//!
//! The canonical digest of a circular record is the lexicographically smallest
//! rotation of the sequence or of its reverse complement. Booth's algorithm
//! finds that rotation in linear time without materializing every rotation.

/// Returns the lexicographically smallest rotation of a sequence.
///
/// The smallest rotation of a string is its representation as a circular
/// sequence and is invariant under rotation of the input. An empty sequence and
/// a one-character sequence are returned unchanged. The function operates on
/// characters, so it is correct for any UTF-8 input; canonical sequences are
/// ASCII IUPAC DNA.
pub fn min_rotation(sequence: &str) -> String {
    let characters: Vec<char> = sequence.chars().collect();
    let length = characters.len();
    if length <= 1 {
        return sequence.to_owned();
    }

    let (mut left, mut right, mut offset) = (0usize, 1usize, 0usize);
    while left < length && right < length && offset < length {
        let left_character = characters[(left + offset) % length];
        let right_character = characters[(right + offset) % length];
        match left_character.cmp(&right_character) {
            std::cmp::Ordering::Equal => offset += 1,
            std::cmp::Ordering::Greater => {
                left += offset + 1;
                if left == right {
                    left += 1;
                }
                offset = 0;
            }
            std::cmp::Ordering::Less => {
                right += offset + 1;
                if left == right {
                    right += 1;
                }
                offset = 0;
            }
        }
    }

    let start = left.min(right);
    characters.iter().cycle().skip(start).take(length).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn naive_min_rotation(sequence: &str) -> String {
        let characters: Vec<char> = sequence.chars().collect();
        if characters.is_empty() {
            return String::new();
        }
        (0..characters.len())
            .map(|start| {
                characters[start..]
                    .iter()
                    .chain(characters[..start].iter())
                    .collect::<String>()
            })
            .min()
            .expect("non-empty rotation list")
    }

    #[test]
    fn small_examples() {
        assert_eq!(min_rotation(""), "");
        assert_eq!(min_rotation("A"), "A");
        assert_eq!(min_rotation("GATC"), "ATCG");
        assert_eq!(min_rotation("AAAA"), "AAAA");
    }

    #[test]
    fn matches_the_naive_definition() {
        let sequences = [
            "GATCCTAGCTAGGCTAAGCTTACCGAT",
            "ATGCGTACGTAGCTAGCTAGCATCGATCGA",
            "TTTTAAAACCCCGGGG",
            "ACGTACGTACGT",
            "NRYSWKMBDHV",
            "GGGGGACGTACGTAAAAA",
        ];
        for sequence in sequences {
            assert_eq!(
                min_rotation(sequence),
                naive_min_rotation(sequence),
                "min rotation of {sequence}"
            );
        }
    }

    #[test]
    fn is_invariant_under_rotation() {
        let sequence = "GATCCTAGCTAGGCTAAGCTTACCGAT";
        let characters: Vec<char> = sequence.chars().collect();
        for start in 0..characters.len() {
            let rotated: String = characters[start..]
                .iter()
                .chain(characters[..start].iter())
                .collect();
            assert_eq!(min_rotation(&rotated), min_rotation(sequence));
        }
    }
}
