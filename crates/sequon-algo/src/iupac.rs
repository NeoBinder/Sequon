//! IUPAC possibility-set helpers shared by translation and restriction analysis.

pub(crate) const A: u8 = 1;
pub(crate) const C: u8 = 2;
pub(crate) const G: u8 = 4;
pub(crate) const T: u8 = 8;

pub(crate) fn mask(base: char) -> Option<u8> {
    match base {
        'A' => Some(A),
        'C' => Some(C),
        'G' => Some(G),
        'T' => Some(T),
        'R' => Some(A | G),
        'Y' => Some(C | T),
        'S' => Some(G | C),
        'W' => Some(A | T),
        'K' => Some(G | T),
        'M' => Some(A | C),
        'B' => Some(C | G | T),
        'D' => Some(A | G | T),
        'H' => Some(A | C | T),
        'V' => Some(A | C | G),
        'N' => Some(A | C | G | T),
        _ => None,
    }
}

pub(crate) fn complement(base: char) -> Option<char> {
    match base {
        'A' => Some('T'),
        'C' => Some('G'),
        'G' => Some('C'),
        'T' => Some('A'),
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
        'N' => Some('N'),
        _ => None,
    }
}

pub(crate) fn concrete_bases(mask: u8) -> Vec<char> {
    let mut bases = Vec::with_capacity(4);
    for (bit, base) in [(A, 'A'), (C, 'C'), (G, 'G'), (T, 'T')] {
        if mask & bit != 0 {
            bases.push(base);
        }
    }
    bases
}

pub(crate) fn expand_codon(codon: &str) -> Option<Vec<[u8; 3]>> {
    let characters: Vec<char> = codon.chars().collect();
    if characters.len() != 3 {
        return None;
    }
    let first = concrete_bases(mask(characters[0])?);
    let second = concrete_bases(mask(characters[1])?);
    let third = concrete_bases(mask(characters[2])?);
    let mut codons = Vec::with_capacity(first.len() * second.len() * third.len());
    for a in first {
        for b in &second {
            for c in &third {
                codons.push([a as u8, *b as u8, *c as u8]);
            }
        }
    }
    Some(codons)
}
