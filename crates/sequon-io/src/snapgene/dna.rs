//! DNA packets: type 0 (text bases, verified) and type 1 (2-bit, hypothesis).
//!
//! Type 0 is verified against the corpus: one flags byte followed by uppercase
//! ASCII bases. Type 1 is the documented hypothesis (design note F-8..F-11):
//! flags byte, big-endian u32 base count, four bases per byte, first base in
//! the high two bits, mapping 0=G, 1=A, 2=T, 3=C, unused low padding bits zero.
//! A real SnapGene type 1 packet has never been observed by this project; the
//! decoder is present so synthetic fixtures can exercise it, and its mapping is
//! not a verified format fact.

use crate::error::ImportError;
use crate::limits::Limits;

use super::container::Packet;

/// One decoded DNA packet.
#[derive(Debug)]
pub(crate) struct DecodedDna {
    /// Decoded and normalized sequence.
    pub sequence: String,
    /// Raw flags byte.
    pub flags: u8,
    /// True when normalization changed the text (lowercase or whitespace).
    pub normalized: bool,
}

/// Decodes a type 0 payload: flags byte plus text bases.
pub(crate) fn decode_type0(
    packet: &Packet<'_>,
    limits: &Limits,
) -> Result<DecodedDna, ImportError> {
    let flags = match packet.payload.first() {
        Some(flags) => *flags,
        None => {
            return Err(ImportError::Malformed {
                code: "DNA_TRUNCATED_PACKET",
                message: "the DNA packet has no flags byte".to_string(),
                position: Some(packet.position()),
            })
        }
    };
    let raw_bytes = &packet.payload[1..];
    let raw = match std::str::from_utf8(raw_bytes) {
        Ok(raw) => raw,
        Err(error) => {
            return Err(ImportError::Malformed {
                code: "DNA_SEQUENCE_INVALID_CHAR",
                message: format!(
                    "the DNA packet contains invalid UTF-8 at byte {}",
                    error.valid_up_to()
                ),
                position: Some(packet.position()),
            })
        }
    };
    for (index, character) in raw.chars().enumerate() {
        if character.is_ascii_digit() {
            return Err(ImportError::Malformed {
                code: "DNA_SEQUENCE_INVALID_CHAR",
                message: format!("digit at character index {index} is not a DNA base"),
                position: Some(packet.position()),
            });
        }
    }
    let sequence = match sequon_algo::normalize_sequence(raw) {
        Ok(sequence) => sequence,
        Err(error) => {
            return Err(ImportError::Malformed {
                code: "DNA_SEQUENCE_INVALID_CHAR",
                message: format!(
                    "character at index {} is outside the IUPAC DNA alphabet",
                    error.index()
                ),
                position: Some(packet.position()),
            })
        }
    };
    if sequence.is_empty() {
        return Err(ImportError::Malformed {
            code: "DNA_SEQUENCE_EMPTY",
            message: "the DNA packet decodes to zero bases".to_string(),
            position: Some(packet.position()),
        });
    }
    if sequence.len() as u64 > limits.max_sequence_len {
        return Err(ImportError::LimitExceeded {
            limit: "max_sequence_len",
            limit_value: limits.max_sequence_len,
            actual: sequence.len() as u64,
            position: Some(packet.position()),
        });
    }
    Ok(DecodedDna {
        normalized: sequence != raw,
        sequence,
        flags,
    })
}

/// Decodes a type 1 2-bit payload (unverified hypothesis, see module docs).
pub(crate) fn decode_type1(
    packet: &Packet<'_>,
    limits: &Limits,
) -> Result<DecodedDna, ImportError> {
    if packet.payload.len() < 5 {
        return Err(compressed_length_invalid(
            packet,
            "the 2-bit packet is shorter than its flags and length field",
        ));
    }
    let flags = packet.payload[0];
    let count = u64::from(u32::from_be_bytes([
        packet.payload[1],
        packet.payload[2],
        packet.payload[3],
        packet.payload[4],
    ]));
    if count > limits.max_sequence_len {
        return Err(ImportError::LimitExceeded {
            limit: "max_sequence_len",
            limit_value: limits.max_sequence_len,
            actual: count,
            position: Some(packet.position()),
        });
    }
    if count == 0 {
        return Err(ImportError::Malformed {
            code: "DNA_SEQUENCE_EMPTY",
            message: "the 2-bit DNA packet decodes to zero bases".to_string(),
            position: Some(packet.position()),
        });
    }
    let packed = &packet.payload[5..];
    let expected = count.div_ceil(4);
    if packed.len() as u64 != expected {
        return Err(compressed_length_invalid(
            packet,
            "the packed payload length does not match the declared base count",
        ));
    }
    let remainder = (count % 4) as u32;
    if remainder != 0 {
        let padding_bits = 2 * (4 - remainder);
        let last = packed[packed.len() - 1];
        let mask = if padding_bits >= 8 {
            0xff
        } else {
            (1_u8 << padding_bits) - 1
        };
        if last & mask != 0 {
            return Err(compressed_length_invalid(
                packet,
                "the final byte has non-zero padding bits",
            ));
        }
    }
    let mut sequence = String::with_capacity(count as usize);
    for byte in packed {
        for shift in [6_u32, 4, 2, 0] {
            if (sequence.len() as u64) == count {
                break;
            }
            let value = (byte >> shift) & 0b11;
            sequence.push(match value {
                0 => 'G',
                1 => 'A',
                2 => 'T',
                _ => 'C',
            });
        }
    }
    if sequence.len() as u64 != count {
        return Err(compressed_length_invalid(
            packet,
            "the packed payload decoded to fewer bases than declared",
        ));
    }
    Ok(DecodedDna {
        normalized: false,
        sequence,
        flags,
    })
}

fn compressed_length_invalid(packet: &Packet<'_>, message: &str) -> ImportError {
    ImportError::Malformed {
        code: "DNA_COMPRESSED_LENGTH_INVALID",
        message: message.to_string(),
        position: Some(packet.position()),
    }
}

#[cfg(test)]
mod tests {
    use super::super::container::Packet;
    use super::*;

    fn packet(payload: &[u8]) -> Packet<'_> {
        Packet {
            index: 1,
            packet_type: 0,
            offset: 19,
            payload,
        }
    }

    #[test]
    fn type0_decodes_and_normalizes() {
        let limits = Limits::default();
        let decoded = decode_type0(&packet(b"\x00ACgt ACGT"), &limits).expect("decode");
        assert_eq!(decoded.sequence, "ACGTACGT");
        assert!(decoded.normalized);
        assert_eq!(decoded.flags, 0);
    }

    #[test]
    fn type0_rejects_digits_and_invalid_bases() {
        let limits = Limits::default();
        let error = decode_type0(&packet(b"\x00ACG1"), &limits).expect_err("digit");
        assert_eq!(error.code(), "DNA_SEQUENCE_INVALID_CHAR");
        let error = decode_type0(&packet(b"\x00ACGZ"), &limits).expect_err("base");
        assert_eq!(error.code(), "DNA_SEQUENCE_INVALID_CHAR");
    }

    #[test]
    fn type0_signals_empty_sequence() {
        let limits = Limits::default();
        let error = decode_type0(&packet(b"\x00"), &limits).expect_err("empty");
        assert_eq!(error.code(), "DNA_SEQUENCE_EMPTY");
    }

    #[test]
    fn type1_hypothesis_round_trips_the_builder_convention() {
        let limits = Limits::default();
        // flags, count=5, packed GATC + G with zero padding.
        let payload = [0x01, 0, 0, 0, 5, 0b00_01_10_11, 0b00_00_00_00];
        let decoded = decode_type1(&packet(&payload), &limits).expect("decode");
        assert_eq!(decoded.sequence, "GATCG");
    }

    #[test]
    fn type1_rejects_non_zero_padding() {
        let limits = Limits::default();
        let payload = [0x01, 0, 0, 0, 5, 0b00_01_10_11, 0b00_00_00_01];
        let error = decode_type1(&packet(&payload), &limits).expect_err("padding");
        assert_eq!(error.code(), "DNA_COMPRESSED_LENGTH_INVALID");
    }

    #[test]
    fn type1_rejects_length_mismatch() {
        let limits = Limits::default();
        let payload = [0x01, 0, 0, 0, 9, 0b00_01_10_11];
        let error = decode_type1(&packet(&payload), &limits).expect_err("length");
        assert_eq!(error.code(), "DNA_COMPRESSED_LENGTH_INVALID");
    }
}
