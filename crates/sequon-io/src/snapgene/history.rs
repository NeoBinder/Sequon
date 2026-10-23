//! History tree blocks (packet types 7 and 29): version rule, decode check,
//! raw preservation. The tree is never mapped into the model (D14).
//!
//! The version rule is documented in design note section 4.1: plain XML below
//! export version 15, LZMA-compressed XML at or above it. The tokenizer and
//! LZMA caps are shared with the other blocks. The decoded content is validated
//! only; its bytes are not interpreted, and the original payload is preserved
//! (or fingerprinted) in the import report.

use sequon_model::JsonValue;

use crate::error::ImportError;

use super::codec;
use super::container::Packet;
use super::state::{json_object, ImportState};
use super::xml;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Encoding {
    PlainXml,
    Lzma,
}

impl Encoding {
    fn label(self) -> &'static str {
        match self {
            Encoding::PlainXml => "xml",
            Encoding::Lzma => "lzma",
        }
    }
}

struct DecodeFailure {
    fatal: Option<ImportError>,
    message: String,
}

/// Decodes and validates one history payload, then preserves its raw bytes.
///
/// Returns the extras.snapgene.history metadata object.
pub(crate) fn process(
    packet: &Packet<'_>,
    export_version: u16,
    state: &mut ImportState<'_>,
) -> Result<JsonValue, ImportError> {
    let primary = if export_version < 15 {
        Encoding::PlainXml
    } else {
        Encoding::Lzma
    };
    let fallback = match primary {
        Encoding::PlainXml => Encoding::Lzma,
        Encoding::Lzma => Encoding::PlainXml,
    };
    let mut used = primary;
    let mut decoded = true;
    // Only the decode path that is kept is charged to the XML byte budget.
    let mut decoded_bytes: Option<u64> = None;
    match decode(packet, primary, state) {
        Ok(bytes) => decoded_bytes = Some(bytes),
        Err(failure) => {
            if let Some(fatal) = failure.fatal {
                return Err(fatal);
            }
            match decode(packet, fallback, state) {
                Ok(bytes) => {
                    used = fallback;
                    decoded_bytes = Some(bytes);
                    state.info(
                        "DNA_HISTORY_DECODE_FALLBACK",
                        "the version-selected history decode failed; the fallback path succeeded",
                        Some(packet.position()),
                    );
                }
                Err(second) => {
                    if let Some(fatal) = second.fatal {
                        return Err(fatal);
                    }
                    decoded = false;
                    state.warning(
                        "DNA_HISTORY_DECODE_FAILED",
                        format!(
                            "the history block could not be decoded ({}); the raw bytes were kept",
                            failure.message
                        ),
                        Some(packet.position()),
                    );
                }
            }
        }
    }
    if let Some(bytes) = decoded_bytes {
        state.commit_xml_bytes(bytes);
    }
    if decoded {
        state.info(
            "DNA_HISTORY_PRESERVED_RAW",
            "the history tree is kept as raw bytes and is not mapped to a family tree",
            Some(packet.position()),
        );
    }
    state.preserve_packet("history_tree", packet);
    Ok(json_object(vec![
        ("encoding".to_owned(), JsonValue::from(used.label())),
        (
            "length".to_owned(),
            JsonValue::from(packet.payload.len() as u64),
        ),
        (
            "digest".to_owned(),
            JsonValue::from(sequon_algo::bytes_digest(packet.payload).as_str()),
        ),
        ("decoded".to_owned(), JsonValue::Bool(decoded)),
    ]))
}

fn decode(
    packet: &Packet<'_>,
    encoding: Encoding,
    state: &ImportState<'_>,
) -> Result<u64, DecodeFailure> {
    match encoding {
        Encoding::PlainXml => {
            let bytes = packet.payload.len() as u64;
            state
                .check_xml_bytes(packet, bytes)
                .map_err(decode_failure)?;
            xml::validate(packet.payload, state.limits(), packet.offset + 5).map_err(|error| {
                DecodeFailure {
                    fatal: None,
                    message: format!("{}: {}", error.code(), error.message()),
                }
            })?;
            Ok(bytes)
        }
        Encoding::Lzma => {
            let decoded = match codec::decompress(packet.payload, state.limits(), packet.position())
            {
                Ok(decoded) => decoded,
                Err(error) => {
                    let fatal = (error.code() == "DNA_DECOMPRESSION_LIMIT").then_some(error);
                    return Err(DecodeFailure {
                        fatal,
                        message: "the LZMA stream could not be decoded".to_string(),
                    });
                }
            };
            let bytes = decoded.len() as u64;
            state
                .check_xml_bytes(packet, bytes)
                .map_err(decode_failure)?;
            xml::validate(&decoded, state.limits(), packet.offset + 5).map_err(|error| {
                DecodeFailure {
                    fatal: None,
                    message: format!("{}: {}", error.code(), error.message()),
                }
            })?;
            Ok(bytes)
        }
    }
}

fn decode_failure(error: ImportError) -> DecodeFailure {
    DecodeFailure {
        fatal: Some(error),
        message: "the decoded XML exceeds the import-wide XML byte budget".to_string(),
    }
}
