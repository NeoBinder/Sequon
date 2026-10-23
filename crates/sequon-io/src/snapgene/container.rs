//! SnapGene container: cookie packet and flat TLV stream.
//!
//! The layout is verified against the calibration corpus recorded in
//! docs/design/snapgene-dna.md section 3: one cookie packet (type 0x09, a
//! big-endian u32 payload length, then \"SnapGene\" plus three big-endian u16
//! values), followed by a flat sequence of TLV packets. The reader never seeks
//! and never computes an offset from a hardcoded constant; it advances by the
//! declared payload lengths.

use crate::error::ImportError;
use crate::limits::Limits;
use crate::report::{Diagnostic, Severity, SourcePosition};

/// Packet type of the cookie that opens every container.
pub(crate) const COOKIE_PACKET_TYPE: u8 = 0x09;

/// Magic prefix of the cookie payload.
const COOKIE_MAGIC: &[u8] = b"SnapGene";

/// Verified cookie payload length: magic plus three u16 values.
const COOKIE_PAYLOAD_LEN: usize = 14;

/// Byte width of one TLV header (type byte plus big-endian u32 length).
const TLV_HEADER_LEN: u64 = 5;

/// Cookie fields. All three values are big-endian u16 as verified on corpus.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Cookie {
    /// Sequence type; 1 is DNA in the corpus.
    pub sequence_type: u16,
    /// Export version; 15 in the corpus.
    pub export_version: u16,
    /// Import version; 19 in the corpus. Read but not interpreted.
    #[allow(dead_code)]
    pub import_version: u16,
}

/// One TLV packet with its position in the file.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Packet<'a> {
    /// 0-based packet index; the cookie is index 0.
    pub index: u32,
    /// Packet type byte.
    pub packet_type: u8,
    /// 0-based byte offset of the packet header.
    pub offset: u64,
    /// Packet payload, excluding the 5-byte header.
    pub payload: &'a [u8],
}

impl Packet<'_> {
    /// Returns the source position of this packet's payload.
    pub(crate) fn position(&self) -> SourcePosition {
        SourcePosition::Snapgene {
            offset: self.offset,
            packet_type: Some(u32::from(self.packet_type)),
            packet_index: Some(self.index),
        }
    }
}

/// A parsed container: the cookie and every TLV packet in file order.
#[derive(Debug)]
pub(crate) struct Container<'a> {
    /// Cookie values.
    pub cookie: Cookie,
    /// TLV packets in file order, excluding the cookie packet.
    pub packets: Vec<Packet<'a>>,
}

/// Reads the cookie and walks the TLV stream.
///
/// Trailing zero bytes after the last packet are treated as padding and
/// ignored. Non-zero trailing bytes shorter than a TLV header are reported as
/// \`DNA_TRAILING_BYTES\` (warning) and ignored; a longer tail is parsed as a
/// packet, so a declared length beyond EOF is a truncated packet, not trailing
/// data.
pub(crate) fn read<'a>(
    bytes: &'a [u8],
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Container<'a>, ImportError> {
    let total = bytes.len() as u64;
    if total < TLV_HEADER_LEN {
        return Err(truncated_header("the file ends before the cookie header"));
    }
    if bytes[0] != COOKIE_PACKET_TYPE {
        return Err(bad_magic());
    }
    let cookie_len = u64::from(u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]));
    if cookie_len < COOKIE_PAYLOAD_LEN as u64 {
        return Err(truncated_header(
            "the cookie payload is shorter than its fields",
        ));
    }
    if TLV_HEADER_LEN + cookie_len > total {
        return Err(truncated_header(
            "the cookie payload reaches past the end of the file",
        ));
    }
    let payload_start = TLV_HEADER_LEN as usize;
    let payload_end = (TLV_HEADER_LEN + cookie_len) as usize;
    let cookie_payload = &bytes[payload_start..payload_end];
    if !cookie_payload.starts_with(COOKIE_MAGIC) {
        return Err(bad_magic());
    }
    let cookie = Cookie {
        sequence_type: u16::from_be_bytes([cookie_payload[8], cookie_payload[9]]),
        export_version: u16::from_be_bytes([cookie_payload[10], cookie_payload[11]]),
        import_version: u16::from_be_bytes([cookie_payload[12], cookie_payload[13]]),
    };

    let mut packets = Vec::new();
    let mut offset = TLV_HEADER_LEN + cookie_len;
    let mut index: u32 = 1;
    while offset < total {
        let remaining = total - offset;
        let tail = &bytes[offset as usize..];
        if tail.iter().all(|byte| *byte == 0) {
            // Zero padding after the last packet is ignored.
            break;
        }
        if remaining < TLV_HEADER_LEN {
            diagnostics.push(
                Diagnostic::new(
                    "DNA_TRAILING_BYTES",
                    Severity::Warning,
                    format!("{remaining} trailing byte(s) after the last packet were ignored"),
                )
                .with_position(position(offset)),
            );
            break;
        }
        let header = &bytes[offset as usize..offset as usize + TLV_HEADER_LEN as usize];
        let packet_type = header[0];
        let length = u64::from(u32::from_be_bytes([
            header[1], header[2], header[3], header[4],
        ]));
        if length > remaining - TLV_HEADER_LEN {
            return Err(ImportError::Malformed {
                code: "DNA_TRUNCATED_PACKET",
                message: format!(
                    "packet {index} (type {packet_type}) declares {length} payload byte(s) but only {} remain",
                    remaining - TLV_HEADER_LEN
                ),
                position: Some(position(offset)),
            });
        }
        if u64::from(index) > limits.max_snapgene_packets {
            return Err(ImportError::LimitExceeded {
                limit: "max_snapgene_packets",
                limit_value: limits.max_snapgene_packets,
                actual: u64::from(index),
                position: Some(position(offset)),
            });
        }
        let payload_begin = (offset + TLV_HEADER_LEN) as usize;
        let payload_end = payload_begin + length as usize;
        packets.push(Packet {
            index,
            packet_type,
            offset,
            payload: &bytes[payload_begin..payload_end],
        });
        offset += TLV_HEADER_LEN + length;
        index = index.saturating_add(1);
    }

    Ok(Container { cookie, packets })
}

fn position(offset: u64) -> SourcePosition {
    SourcePosition::Snapgene {
        offset,
        packet_type: None,
        packet_index: None,
    }
}

fn truncated_header(message: &str) -> ImportError {
    ImportError::Malformed {
        code: "DNA_TRUNCATED_HEADER",
        message: message.to_string(),
        position: Some(position(0)),
    }
}

fn bad_magic() -> ImportError {
    ImportError::Malformed {
        code: "DNA_BAD_MAGIC",
        message: "the input does not start with the SnapGene cookie packet".to_string(),
        position: Some(position(0)),
    }
}
