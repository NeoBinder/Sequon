//! Import state shared by the SnapGene packet handlers.
//!
//! The state owns the diagnostic, dropped and preserved collections so every
//! handler reports loss in one place and in file order. Diagnostics are sorted
//! once before the report is returned (ImportReport::sort_diagnostics).

use sequon_model::JsonValue;

use crate::error::ImportError;
use crate::limits::Limits;
use crate::report::{Diagnostic, Dropped, Preserved, PreservedEncoding, Severity, SourcePosition};

use super::container::Packet;

/// Record index used by this single-record format.
pub(crate) const RECORD_INDEX: u32 = 0;

/// Producer name recorded in provenance and in the import report.
pub(crate) const PRODUCER_NAME: &str = "sequon-io";

/// Mutable import state.
pub(crate) struct ImportState<'a> {
    limits: &'a Limits,
    preserve_unhandled: bool,
    /// Diagnostics in emission order; sorted before the report is returned.
    pub diagnostics: Vec<Diagnostic>,
    /// Skipped blocks and features with fingerprints.
    pub dropped: Vec<Dropped>,
    /// Raw blocks kept verbatim.
    pub preserved: Vec<Preserved>,
    preserved_bytes: u64,
    /// Total decoded XML bytes across all XML blocks of this import.
    xml_bytes: u64,
}

impl<'a> ImportState<'a> {
    /// Creates state for one import.
    pub(crate) fn new(limits: &'a Limits, preserve_unhandled: bool) -> Self {
        Self {
            limits,
            preserve_unhandled,
            diagnostics: Vec::new(),
            dropped: Vec::new(),
            preserved: Vec::new(),
            preserved_bytes: 0,
            xml_bytes: 0,
        }
    }

    /// Returns the configured limits.
    pub(crate) fn limits(&self) -> &Limits {
        self.limits
    }

    /// Checks the accumulated decoded-XML byte budget without committing it.
    ///
    /// Used when a candidate decode may still be discarded (the history
    /// fallback path): the budget is charged only for the path that is kept.
    pub(crate) fn check_xml_bytes(
        &self,
        packet: &Packet<'_>,
        bytes: u64,
    ) -> Result<(), ImportError> {
        let total = self.xml_bytes.saturating_add(bytes);
        if total > self.limits.max_xml_bytes {
            return Err(ImportError::LimitExceeded {
                limit: "max_xml_bytes",
                limit_value: self.limits.max_xml_bytes,
                actual: total,
                position: Some(packet.position()),
            });
        }
        Ok(())
    }

    /// Charges decoded XML bytes to the import-wide budget.
    pub(crate) fn commit_xml_bytes(&mut self, bytes: u64) {
        self.xml_bytes = self.xml_bytes.saturating_add(bytes);
    }

    /// Checks and charges one XML block to the import-wide budget.
    pub(crate) fn reserve_xml_bytes(
        &mut self,
        packet: &Packet<'_>,
        bytes: u64,
    ) -> Result<(), ImportError> {
        self.check_xml_bytes(packet, bytes)?;
        self.commit_xml_bytes(bytes);
        Ok(())
    }

    /// Appends an informational diagnostic.
    pub(crate) fn info(
        &mut self,
        code: &str,
        message: impl Into<String>,
        position: Option<SourcePosition>,
    ) {
        self.push(code, Severity::Info, message, position);
    }

    /// Appends a warning diagnostic.
    pub(crate) fn warning(
        &mut self,
        code: &str,
        message: impl Into<String>,
        position: Option<SourcePosition>,
    ) {
        self.push(code, Severity::Warning, message, position);
    }

    fn push(
        &mut self,
        code: &str,
        severity: Severity,
        message: impl Into<String>,
        position: Option<SourcePosition>,
    ) {
        self.diagnostics.push(
            Diagnostic::new(code, severity, message)
                .with_position_optional(position)
                .with_record_index(RECORD_INDEX),
        );
    }

    /// Preserves a packet payload verbatim, or fingerprints it when the caller
    /// disabled preservation or the preserved-byte budget is exhausted.
    pub(crate) fn preserve_packet(&mut self, what: &str, packet: &Packet<'_>) -> bool {
        let length = packet.payload.len() as u64;
        if !self.preserve_unhandled {
            self.drop_packet(what, "not_requested", packet);
            return false;
        }
        if self.preserved_bytes.saturating_add(length) > self.limits.max_preserved_block_bytes {
            self.drop_packet(what, "size_limit", packet);
            return false;
        }
        self.preserved.push(Preserved {
            what: what.to_owned(),
            encoding: PreservedEncoding::Base64,
            data: base64_encode(packet.payload),
            packet_type: Some(u32::from(packet.packet_type)),
            offset: Some(packet.offset),
            length: Some(length),
            digest: Some(sequon_algo::bytes_digest(packet.payload)),
            record_index: Some(RECORD_INDEX),
        });
        self.preserved_bytes = self.preserved_bytes.saturating_add(length);
        true
    }

    /// Records a skipped packet with a fingerprint.
    pub(crate) fn drop_packet(&mut self, what: &str, reason: &str, packet: &Packet<'_>) {
        self.dropped.push(Dropped {
            what: what.to_owned(),
            reason: reason.to_owned(),
            record_index: Some(RECORD_INDEX),
            position: Some(packet.position()),
            packet_type: Some(u32::from(packet.packet_type)),
            offset: Some(packet.offset),
            length: Some(packet.payload.len() as u64),
            digest: Some(sequon_algo::bytes_digest(packet.payload)),
            feature_key: None,
            location_text: None,
        });
    }

    /// Records a skipped feature or primer.
    pub(crate) fn drop_feature(
        &mut self,
        reason: &str,
        packet: &Packet<'_>,
        feature_key: &str,
        location_text: &str,
    ) {
        self.dropped.push(Dropped {
            what: "feature".to_owned(),
            reason: reason.to_owned(),
            record_index: Some(RECORD_INDEX),
            position: Some(packet.position()),
            packet_type: Some(u32::from(packet.packet_type)),
            offset: Some(packet.offset),
            length: None,
            digest: None,
            feature_key: Some(feature_key.to_owned()),
            location_text: Some(location_text.to_owned()),
        });
    }
}

/// Trait used to attach an optional position without an extra branch.
trait WithOptionalPosition {
    fn with_position_optional(self, position: Option<SourcePosition>) -> Self;
}

impl WithOptionalPosition for Diagnostic {
    fn with_position_optional(self, position: Option<SourcePosition>) -> Self {
        match position {
            Some(position) => self.with_position(position),
            None => self,
        }
    }
}

/// Builds a JSON object from ordered key/value pairs.
pub(crate) fn json_object(pairs: Vec<(String, JsonValue)>) -> JsonValue {
    JsonValue::Object(pairs.into_iter().collect())
}

/// Encodes bytes as standard padded base64 (no line breaks).
pub(crate) fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        output.push(char::from(TABLE[usize::from(first >> 2)]));
        output.push(char::from(
            TABLE[usize::from(((first & 0b11) << 4) | (second >> 4))],
        ));
        if chunk.len() > 1 {
            output.push(char::from(
                TABLE[usize::from(((second & 0b1111) << 2) | (third >> 6))],
            ));
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(char::from(TABLE[usize::from(third & 0b11_1111)]));
        } else {
            output.push('=');
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode(&[0, 255, 128]), "AP+A");
    }
}
