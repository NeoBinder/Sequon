//! Deterministic builder for synthetic SnapGene `.dna` containers.
//!
//! This module exists to produce the committed fixtures under
//! `testdata/snapgene/`. It is a fixture generator, not a format writer: the
//! adapter is import-only (D14) and nothing in this module is reachable from
//! `sequon-io`. It must never grow into a production `write_snapgene`.
//!
//! # What it models
//!
//! The container layout is the one recorded in `docs/design/snapgene-dna.md`
//! and calibrated against the local corpus:
//!
//! - cookie packet: type `0x09`, big-endian payload length, payload
//!   `SnapGene` plus three big-endian `u16` values (sequence type, export
//!   version, import version);
//! - a flat TLV stream (type byte, big-endian `u32` length, payload), emitted
//!   in the order the caller pushes it, so packet order is preserved;
//! - type 0 DNA (flags byte plus the bases as written), type 10 features XML,
//!   type 6 notes XML, type 8 properties XML, type 5 primers XML, type 3 and
//!   type 13 metadata payloads, type 16 traces and unknown packet types;
//! - optional type 1 2-bit DNA and type 7/29 history payloads.
//!
//! # Determinism
//!
//! The builder reads no clock, no randomness, no environment and no
//! filesystem. Every call with the same builder state produces the same bytes.
//! LZMA compression uses the documented LZMA-alone framing with the unpacked
//! size written into the header, so `lzma-rs` produces one deterministic
//! stream per input. `tests/snapgene_fixtures.rs` asserts that regenerating
//! the catalogue reproduces the committed bytes and their sha256 digests.
//!
//! # Verified facts versus hypotheses
//!
//! The container, cookie, type 0 DNA, notes/properties/features XML shape and
//! the 1-based closed `range="A-B"` convention are corpus-verified (see the
//! design note, section 10). The following are **builder conventions** that
//! realise a documented hypothesis and do not confirm it:
//!
//! - 2-bit DNA: mapping `0=G, 1=A, 2=T, 3=C`, first base in the high two bits,
//!   then a big-endian `u32` base count after the flags byte, final-byte
//!   padding bits zero. Facts F-8 to F-11 stay unverified until a real
//!   SnapGene file is compared.
//! - history framing: plain XML for `export_version < 15`, LZMA-alone for
//!   `export_version >= 15` (F-20). The embedded XML is a synthetic
//!   placeholder, not a SnapGene history tree.
//! - type 3 and type 13 payloads reproduce the observed framing but not the
//!   undocumented field semantics (F-26, F-27).

use std::fmt;

use lzma_rs::compress::{Options as LzmaOptions, UnpackedSize};
use lzma_rs::lzma_compress_with_options;

/// Packet type of the cookie packet that opens every `.dna` container.
pub const COOKIE_PACKET_TYPE: u8 = 0x09;

/// Packet type of an uncompressed DNA packet (flags byte, then text bases).
pub const DNA_PACKET_TYPE: u8 = 0;

/// Packet type of a 2-bit compressed DNA packet (builder convention).
pub const COMPRESSED_DNA_PACKET_TYPE: u8 = 1;

/// Packet type of the restriction-enzyme recognition-site list.
pub const RESTRICTION_SITES_PACKET_TYPE: u8 = 3;

/// Packet type of the primers XML block.
pub const PRIMERS_PACKET_TYPE: u8 = 5;

/// Packet type of the notes XML block.
pub const NOTES_PACKET_TYPE: u8 = 6;

/// Packet type of the history tree block.
pub const HISTORY_PACKET_TYPE: u8 = 7;

/// Packet type of the sequence-properties XML block.
pub const PROPERTIES_PACKET_TYPE: u8 = 8;

/// Packet type of the features XML block.
pub const FEATURES_PACKET_TYPE: u8 = 10;

/// Packet type of the enzyme-settings metadata block.
pub const ENZYME_SETTINGS_PACKET_TYPE: u8 = 13;

/// Packet type of the binary trace container.
pub const TRACE_PACKET_TYPE: u8 = 16;

/// Packet type of the alignable-sequences XML block.
pub const ALIGNABLE_PACKET_TYPE: u8 = 17;

/// Packet type of the history-modifier block.
pub const HISTORY_MODIFIER_PACKET_TYPE: u8 = 29;

/// Cookie sequence type that selects `kind = dna` (corpus-verified).
pub const SEQUENCE_TYPE_DNA: u16 = 1;

/// Cookie sequence type used by the synthetic non-DNA fixture.
///
/// The numeric value for RNA is **not** verified (design note F-4). The lead
/// decision of 2026-10-21 is strict: only `1` means DNA, and every other
/// value maps to `DNA_UNKNOWN_SEQ_TYPE` until a real non-DNA file verifies
/// the enum. `2` is therefore a placeholder, not an RNA value; fixtures
/// `131-rna-seq-type.dna` and `132-unknown-seq-type.dna` exercise two
/// different unknown values.
pub const SEQUENCE_TYPE_NON_DNA_PLACEHOLDER: u16 = 2;

/// Cookie sequence type used by the synthetic unknown-type fixture.
///
/// `0` is outside the observed enum and is used only to trigger
/// `DNA_UNKNOWN_SEQ_TYPE`; it is not a format fact.
pub const SEQUENCE_TYPE_UNKNOWN_PLACEHOLDER: u16 = 0;

/// Byte layout of the cookie payload: `SnapGene` plus three `u16`.
const COOKIE_MAGIC: &[u8] = b"SnapGene";

/// Number of bytes in the cookie payload (verified: 14).
const COOKIE_PAYLOAD_LEN: usize = 14;

/// Error returned by builder operations that cannot represent their input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuilderError {
    /// A base outside `GATC` was passed to the 2-bit encoder.
    TwoBitBase {
        /// Byte offset of the offending character in the bases string.
        offset: usize,
        /// The offending character (never the full sequence).
        base: char,
    },
    /// The base count does not fit the builder's `u32` length field.
    TooManyBases {
        /// Number of bases supplied.
        count: usize,
    },
}

impl fmt::Display for BuilderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TwoBitBase { offset, base } => {
                write!(
                    formatter,
                    "2-bit DNA cannot encode {base:?} at byte {offset}"
                )
            }
            Self::TooManyBases { count } => {
                write!(formatter, "2-bit DNA base count {count} exceeds u32")
            }
        }
    }
}

impl std::error::Error for BuilderError {}

/// One feature segment, expressed as a 1-based closed range.
///
/// The source convention is verified: `start=158, end=203` becomes
/// `range="158-203"`, which the reader converts to 0-based half-open
/// `start0 = start - 1`, `end0 = end`.
#[derive(Debug, Clone)]
pub struct Segment {
    start: u64,
    end: u64,
    attributes: Vec<(String, String)>,
}

impl Segment {
    /// Creates a segment from a 1-based closed range.
    #[must_use]
    pub fn new(start: u64, end: u64) -> Self {
        Self {
            start,
            end,
            attributes: Vec::new(),
        }
    }

    /// Adds an XML attribute to the `Segment` element (for example `color`).
    #[must_use]
    pub fn attribute(mut self, name: &str, value: &str) -> Self {
        self.attributes.push((name.to_owned(), value.to_owned()));
        self
    }

    /// Returns the 1-based closed start.
    #[must_use]
    pub fn start(&self) -> u64 {
        self.start
    }

    /// Returns the 1-based closed end.
    #[must_use]
    pub fn end(&self) -> u64 {
        self.end
    }
}

/// A qualifier value: the `V@text` or `V@int` form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QualifierValue {
    /// `<V text="..."/>`; text is XML-escaped by the builder.
    Text(String),
    /// `<V int="..."/>`; the canonical model stores the decimal string.
    Int(i64),
}

/// One `<Q>` qualifier.
#[derive(Debug, Clone)]
pub struct Qualifier {
    name: String,
    value: QualifierValue,
}

impl Qualifier {
    /// Creates a text-valued qualifier.
    #[must_use]
    pub fn text(name: &str, value: &str) -> Self {
        Self {
            name: name.to_owned(),
            value: QualifierValue::Text(value.to_owned()),
        }
    }

    /// Creates an integer-valued qualifier.
    #[must_use]
    pub fn int(name: &str, value: i64) -> Self {
        Self {
            name: name.to_owned(),
            value: QualifierValue::Int(value),
        }
    }
}

/// One `<Feature>` element.
///
/// `key` becomes `Feature@type`, `name` becomes `Feature@name` and
/// `directionality` becomes `Feature@directionality` (absent, `1` forward,
/// `2` reverse). Everything else is kept as ordered source attributes.
#[derive(Debug, Clone)]
pub struct Feature {
    key: String,
    name: Option<String>,
    directionality: Option<u8>,
    segments: Vec<Segment>,
    attributes: Vec<(String, String)>,
    qualifiers: Vec<Qualifier>,
}

impl Feature {
    /// Creates a feature from its key and segments.
    #[must_use]
    pub fn new(key: &str, segments: Vec<Segment>) -> Self {
        Self {
            key: key.to_owned(),
            name: None,
            directionality: None,
            segments,
            attributes: Vec::new(),
            qualifiers: Vec::new(),
        }
    }

    /// Sets the display name (`Feature@name`). Empty names are kept as empty
    /// attributes so a fixture can exercise the "empty becomes null" branch.
    #[must_use]
    pub fn name(mut self, name: &str) -> Self {
        self.name = Some(name.to_owned());
        self
    }

    /// Sets `Feature@directionality`: `1` is forward, `2` is reverse.
    #[must_use]
    pub fn directionality(mut self, directionality: u8) -> Self {
        self.directionality = Some(directionality);
        self
    }

    /// Adds a feature-level source attribute (for example `recentID`).
    #[must_use]
    pub fn attribute(mut self, name: &str, value: &str) -> Self {
        self.attributes.push((name.to_owned(), value.to_owned()));
        self
    }

    /// Adds a text-valued qualifier.
    #[must_use]
    pub fn text_qualifier(mut self, name: &str, value: &str) -> Self {
        self.qualifiers.push(Qualifier::text(name, value));
        self
    }

    /// Adds an integer-valued qualifier.
    #[must_use]
    pub fn int_qualifier(mut self, name: &str, value: i64) -> Self {
        self.qualifiers.push(Qualifier::int(name, value));
        self
    }
}

/// Renders a features XML payload (`<Features>`) from feature descriptions.
///
/// Segments are rendered in the order given; nothing is sorted. A feature with
/// no segments renders without a `Segment` element, which the reader must
/// treat as malformed metadata.
#[must_use]
pub fn features_xml(features: &[Feature]) -> String {
    let mut xml = String::from("<?xml version=\"1.0\"?>\n");
    xml.push_str(&format!("<Features nextValidID=\"{}\">\n", features.len()));
    for (index, feature) in features.iter().enumerate() {
        xml.push_str("  <Feature");
        xml.push_str(&attribute_pair("recentID", &index.to_string()));
        if let Some(name) = &feature.name {
            xml.push_str(&attribute_pair("name", name));
        }
        xml.push_str(&attribute_pair("type", &feature.key));
        if let Some(directionality) = feature.directionality {
            xml.push_str(&attribute_pair(
                "directionality",
                &directionality.to_string(),
            ));
        }
        for (name, value) in &feature.attributes {
            xml.push_str(&attribute_pair(name, value));
        }
        if feature.segments.is_empty() && feature.qualifiers.is_empty() {
            xml.push_str("/>\n");
            continue;
        }
        xml.push_str(">\n");
        for segment in &feature.segments {
            xml.push_str("    <Segment");
            xml.push_str(&attribute_pair(
                "range",
                &format!("{}-{}", segment.start, segment.end),
            ));
            for (name, value) in &segment.attributes {
                xml.push_str(&attribute_pair(name, value));
            }
            xml.push_str("/>\n");
        }
        for qualifier in &feature.qualifiers {
            xml.push_str("    <Q");
            xml.push_str(&attribute_pair("name", &qualifier.name));
            xml.push('>');
            match &qualifier.value {
                QualifierValue::Text(text) => {
                    xml.push_str("<V");
                    xml.push_str(&attribute_pair("text", text));
                    xml.push_str("/>");
                }
                QualifierValue::Int(value) => {
                    xml.push_str("<V");
                    xml.push_str(&attribute_pair("int", &value.to_string()));
                    xml.push_str("/>");
                }
            }
            xml.push_str("</Q>\n");
        }
        xml.push_str("  </Feature>\n");
    }
    xml.push_str("</Features>\n");
    xml
}

/// One ordered notes field (`<Name attributes>value</Name>`).
#[derive(Debug, Clone)]
pub struct NoteField {
    /// Element name, for example `CustomMapLabel`.
    pub name: String,
    /// Element text content.
    pub value: String,
    /// Ordered XML attributes of the element (for example `UTC`).
    pub attributes: Vec<(String, String)>,
}

/// Ordered `Notes` XML payload.
#[derive(Debug, Clone, Default)]
pub struct Notes {
    fields: Vec<NoteField>,
}

impl Notes {
    /// Creates an empty notes block.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a field without attributes.
    pub fn field(&mut self, name: &str, value: &str) -> &mut Self {
        self.fields.push(NoteField {
            name: name.to_owned(),
            value: value.to_owned(),
            attributes: Vec::new(),
        });
        self
    }

    /// Appends a field with ordered attributes.
    pub fn field_with_attributes(
        &mut self,
        name: &str,
        value: &str,
        attributes: &[(&str, &str)],
    ) -> &mut Self {
        self.fields.push(NoteField {
            name: name.to_owned(),
            value: value.to_owned(),
            attributes: attributes
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect(),
        });
        self
    }

    /// Renders the `<Notes>` payload with fields in insertion order.
    #[must_use]
    pub fn render_xml(&self) -> String {
        let mut xml = String::from("<?xml version=\"1.0\"?>\n<Notes>\n");
        for field in &self.fields {
            xml.push_str("  <");
            xml.push_str(&field.name);
            for (name, value) in &field.attributes {
                xml.push_str(&attribute_pair(name, value));
            }
            xml.push('>');
            xml.push_str(&escape_text(&field.value));
            xml.push_str("</");
            xml.push_str(&field.name);
            xml.push_str(">\n");
        }
        xml.push_str("</Notes>\n");
        xml
    }
}

/// Ordered `AdditionalSequenceProperties` XML payload.
#[derive(Debug, Clone, Default)]
pub struct Properties {
    fields: Vec<(String, String)>,
}

impl Properties {
    /// Creates an empty properties block.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a scalar property in source order.
    pub fn field(&mut self, name: &str, value: &str) -> &mut Self {
        self.fields.push((name.to_owned(), value.to_owned()));
        self
    }

    /// Renders the `<AdditionalSequenceProperties>` payload.
    #[must_use]
    pub fn render_xml(&self) -> String {
        let mut xml = String::from("<?xml version=\"1.0\"?>\n<AdditionalSequenceProperties>\n");
        for (name, value) in &self.fields {
            xml.push_str("  <");
            xml.push_str(name);
            xml.push('>');
            xml.push_str(&escape_text(value));
            xml.push_str("</");
            xml.push_str(name);
            xml.push_str(">\n");
        }
        xml.push_str("</AdditionalSequenceProperties>\n");
        xml
    }
}

/// Renders the corpus-verified empty `Primers` document with settings.
///
/// The non-empty `<Primer>` element shape is unverified (F-17), so the
/// builder deliberately offers no helper for it; a future fixture can pass raw
/// XML through `ContainerBuilder::push_raw` once the shape is confirmed.
#[must_use]
pub fn empty_primers_xml() -> String {
    let mut xml = String::from("<?xml version=\"1.0\"?>\n");
    xml.push_str("<Primers nextValidID=\"0\">\n");
    xml.push_str(
        "  <HybridizationParams minContinuousMatchLen=\"10\" allowMismatch=\"1\" \
         minMeltingTemperature=\"40\" showAdditionalFivePrimeMatches=\"1\" \
         minimumFivePrimeAnnealing=\"15\"/>\n",
    );
    xml.push_str("</Primers>\n");
    xml
}

/// Builds a type 3 restriction-site payload: version 1, big-endian list
/// length, comma-separated IUPAC sites, then opaque remainder bytes.
///
/// The framing is corpus-verified (F-26); the trailing bytes are opaque and
/// are only preserved by the reader.
#[must_use]
pub fn restriction_sites_payload(sites: &[&str], opaque_tail: &[u8]) -> Vec<u8> {
    let list = sites.join(",");
    let mut payload = Vec::with_capacity(5 + list.len() + opaque_tail.len());
    payload.push(1);
    payload.extend_from_slice(&u32::try_from(list.len()).unwrap_or(u32::MAX).to_be_bytes());
    payload.extend_from_slice(list.as_bytes());
    payload.extend_from_slice(opaque_tail);
    payload
}

/// Builds a type 13 enzyme-settings payload reproducing the observed framing.
///
/// The 18-byte header and the remainder semantics are unverified (F-27); the
/// builder reproduces the observed byte positions (label at offset 18) and
/// pads deterministically.
#[must_use]
pub fn enzyme_settings_payload(label: &str) -> Vec<u8> {
    const HEADER: [u8; 18] = [
        0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x4b, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x03,
    ];
    let mut payload = Vec::with_capacity(HEADER.len() + label.len() + 8);
    payload.extend_from_slice(&HEADER);
    payload.extend_from_slice(label.as_bytes());
    payload.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x00, 0x00]);
    payload
}

/// Builds an LZMA-alone header that declares an unpacked size far above the
/// configured output cap, followed by a stub stream.
///
/// This is the decompression-limit fixture payload. The design note
/// (section 2.8) requires the reader to reject a declared size above
/// `max_lzma_output_bytes` from the 13-byte header, before allocating or
/// decoding, with `DNA_DECOMPRESSION_LIMIT`. The declared-size pre-check is
/// part of the F-20 hypothesis: if the payload is not an LZMA-alone stream,
/// the reader can only discover that by attempting a decode, and the fixture
/// then documents a contract gap rather than a confirmed fact.
///
/// A ratio-only bomb cannot be produced with `lzma-rs` 0.3.0: measured on
/// this toolchain it compresses a 1 MiB repeated payload to about 26 KiB
/// (roughly 40:1), below the 200:1 ratio cap at every practical size. The
/// declared-size pre-check is the constructible variant of the same limit.
#[must_use]
pub fn lzma_declared_size_bomb(declared_unpacked_size: u64) -> Vec<u8> {
    // LZMA-alone header: properties byte, 4-byte LE dictionary size,
    // 8-byte LE unpacked size. Dictionary stays at the 8 MiB cap.
    let mut payload = Vec::with_capacity(13 + 8);
    payload.push(0x5d);
    payload.extend_from_slice(&8_388_608_u32.to_le_bytes());
    payload.extend_from_slice(&declared_unpacked_size.to_le_bytes());
    // A short stub so the payload is not empty; a conforming reader never
    // reaches it because the header check fires first.
    payload.extend_from_slice(&[0x00; 8]);
    payload
}

/// Compresses bytes as an LZMA-alone stream with the unpacked size in the
/// header, matching the history framing documented for `export_version >= 15`.
#[must_use]
pub fn lzma_compress_alone(data: &[u8]) -> Vec<u8> {
    let options = LzmaOptions {
        unpacked_size: UnpackedSize::WriteToHeader(Some(data.len() as u64)),
    };
    let mut input = data;
    let mut output = Vec::new();
    lzma_compress_with_options(&mut input, &mut output, &options)
        .expect("in-memory LZMA compression of a byte slice cannot fail on IO");
    output
}

/// Builder for a synthetic `.dna` container.
#[derive(Debug, Clone)]
pub struct ContainerBuilder {
    sequence_type: u16,
    export_version: u16,
    import_version: u16,
    packets: Vec<(u8, Vec<u8>)>,
}

impl ContainerBuilder {
    /// Creates a builder with the default import version `19`.
    #[must_use]
    pub fn new(sequence_type: u16, export_version: u16) -> Self {
        Self {
            sequence_type,
            export_version,
            import_version: 19,
            packets: Vec::new(),
        }
    }

    /// Overrides the cookie import version (default `19`, corpus value).
    #[must_use]
    pub fn import_version(mut self, import_version: u16) -> Self {
        self.import_version = import_version;
        self
    }

    /// Appends a packet with an explicit type and opaque payload, preserving
    /// call order. This is the escape hatch for negative and future fixtures.
    pub fn push_raw(&mut self, packet_type: u8, payload: impl Into<Vec<u8>>) -> &mut Self {
        self.packets.push((packet_type, payload.into()));
        self
    }

    /// Appends a type 0 DNA packet: a flags byte followed by the bases as
    /// written. Fixtures normally pass uppercase `ACGT...`; the lowercase
    /// fixture deliberately passes lowercase to exercise normalisation.
    pub fn push_dna(&mut self, flags: u8, bases: &str) -> &mut Self {
        let mut payload = Vec::with_capacity(1 + bases.len());
        payload.push(flags);
        payload.extend_from_slice(bases.as_bytes());
        self.push_raw(DNA_PACKET_TYPE, payload)
    }

    /// Appends a type 1 2-bit DNA packet using the builder convention
    /// described in the module documentation (unverified hypothesis).
    ///
    /// # Errors
    ///
    /// Returns `BuilderError::TwoBitBase` for a base outside `GATC` and
    /// `BuilderError::TooManyBases` when the count does not fit a `u32`.
    pub fn push_dna_2bit(&mut self, flags: u8, bases: &str) -> Result<&mut Self, BuilderError> {
        let payload = encode_2bit(flags, bases)?;
        self.push_raw(COMPRESSED_DNA_PACKET_TYPE, payload);
        Ok(self)
    }

    /// Appends a features XML packet (type 10).
    pub fn push_features(&mut self, features: &[Feature]) -> &mut Self {
        self.push_raw(FEATURES_PACKET_TYPE, features_xml(features).into_bytes())
    }

    /// Appends a notes XML packet (type 6).
    pub fn push_notes(&mut self, notes: &Notes) -> &mut Self {
        self.push_raw(NOTES_PACKET_TYPE, notes.render_xml().into_bytes())
    }

    /// Appends a properties XML packet (type 8).
    pub fn push_properties(&mut self, properties: &Properties) -> &mut Self {
        self.push_raw(PROPERTIES_PACKET_TYPE, properties.render_xml().into_bytes())
    }

    /// Appends the corpus-verified empty primers packet (type 5).
    pub fn push_empty_primers(&mut self) -> &mut Self {
        self.push_raw(PRIMERS_PACKET_TYPE, empty_primers_xml().into_bytes())
    }

    /// Appends a type 5 primers packet containing caller-supplied XML.
    ///
    /// The non-empty `<Primer>` shape is unverified (F-17), so the builder
    /// does not synthesise one. Fixture `030-primers.dna` supplies the
    /// hypothesis-level document explicitly and marks it as unverified in
    /// `testdata/README.md`; the M4 reader maps it generically (name plus a
    /// `Segment` range) and must not treat the shape as confirmed.
    pub fn push_primers_raw(&mut self, xml: &str) -> &mut Self {
        self.push_raw(PRIMERS_PACKET_TYPE, xml.as_bytes().to_vec())
    }

    /// Appends a history packet (type 7) whose payload is selected by the
    /// builder's `export_version`: plain XML below 15, LZMA-alone at or above
    /// it (design note section 4.1).
    pub fn push_history_xml(&mut self, xml: &str) -> &mut Self {
        let payload = if self.export_version < 15 {
            xml.as_bytes().to_vec()
        } else {
            lzma_compress_alone(xml.as_bytes())
        };
        self.push_raw(HISTORY_PACKET_TYPE, payload)
    }

    /// Appends a history packet (type 7) with a caller-supplied payload, for
    /// example a deliberately over-compressed bomb.
    pub fn push_history_raw(&mut self, payload: impl Into<Vec<u8>>) -> &mut Self {
        self.push_raw(HISTORY_PACKET_TYPE, payload)
    }

    /// Appends a history-modifier packet (type 29) whose payload uses the same
    /// export-version rule as the history tree: plain XML below 15, LZMA-alone
    /// at or above it.
    ///
    /// Whether type 29 obeys the type-7 version threshold is unverified
    /// (F-21); this helper encodes the documented hypothesis only. The payload
    /// XML is synthetic and is never mapped into the model.
    pub fn push_history_modifier_xml(&mut self, xml: &str) -> &mut Self {
        let payload = if self.export_version < 15 {
            xml.as_bytes().to_vec()
        } else {
            lzma_compress_alone(xml.as_bytes())
        };
        self.push_raw(HISTORY_MODIFIER_PACKET_TYPE, payload)
    }

    /// Appends a type 3 restriction-site packet.
    pub fn push_restriction_sites(&mut self, sites: &[&str], opaque_tail: &[u8]) -> &mut Self {
        self.push_raw(
            RESTRICTION_SITES_PACKET_TYPE,
            restriction_sites_payload(sites, opaque_tail),
        )
    }

    /// Appends a type 13 enzyme-settings packet.
    pub fn push_enzyme_settings(&mut self, label: &str) -> &mut Self {
        self.push_raw(ENZYME_SETTINGS_PACKET_TYPE, enzyme_settings_payload(label))
    }

    /// Appends a type 16 trace packet with opaque bytes.
    pub fn push_trace(&mut self, payload: impl Into<Vec<u8>>) -> &mut Self {
        self.push_raw(TRACE_PACKET_TYPE, payload)
    }

    /// Appends a type 17 alignable-sequences packet with caller-supplied XML.
    ///
    /// The element shape is unverified and the reader does not interpret the
    /// block; its payload is preserved (or fingerprinted) with
    /// `DNA_PACKET_IGNORED` (info). Fixture `063-alignable-sequences.dna`
    /// supplies a documented synthetic document.
    pub fn push_alignable_raw(&mut self, xml: &str) -> &mut Self {
        self.push_raw(ALIGNABLE_PACKET_TYPE, xml.as_bytes().to_vec())
    }

    /// Serialises the container: cookie packet followed by the TLV stream in
    /// insertion order.
    #[must_use]
    pub fn build(&self) -> Vec<u8> {
        let mut cookie_payload = Vec::with_capacity(COOKIE_PAYLOAD_LEN);
        cookie_payload.extend_from_slice(COOKIE_MAGIC);
        cookie_payload.extend_from_slice(&self.sequence_type.to_be_bytes());
        cookie_payload.extend_from_slice(&self.export_version.to_be_bytes());
        cookie_payload.extend_from_slice(&self.import_version.to_be_bytes());

        let mut output = Vec::new();
        write_packet(&mut output, COOKIE_PACKET_TYPE, &cookie_payload);
        for (packet_type, payload) in &self.packets {
            write_packet(&mut output, *packet_type, payload);
        }
        output
    }

    /// Serialises the container and keeps only the first `keep_bytes` bytes.
    ///
    /// Truncating inside the last packet yields the declared-length-beyond-EOF
    /// shape the reader must reject with `DNA_TRUNCATED_PACKET`.
    #[must_use]
    pub fn build_truncated(&self, keep_bytes: usize) -> Vec<u8> {
        let mut bytes = self.build();
        bytes.truncate(keep_bytes.min(bytes.len()));
        bytes
    }
}

fn write_packet(output: &mut Vec<u8>, packet_type: u8, payload: &[u8]) {
    output.push(packet_type);
    output.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    output.extend_from_slice(payload);
}

fn encode_2bit(flags: u8, bases: &str) -> Result<Vec<u8>, BuilderError> {
    let count = u32::try_from(bases.len())
        .map_err(|_| BuilderError::TooManyBases { count: bases.len() })?;
    let mut packed = Vec::with_capacity(bases.len().div_ceil(4));
    let mut accumulator = 0_u8;
    let mut filled = 0_u8;
    for (offset, base) in bases.chars().enumerate() {
        let value = match base.to_ascii_uppercase() {
            'G' => 0_u8,
            'A' => 1,
            'T' => 2,
            'C' => 3,
            other => {
                return Err(BuilderError::TwoBitBase {
                    offset,
                    base: other,
                })
            }
        };
        accumulator = (accumulator << 2) | value;
        filled += 1;
        if filled == 4 {
            packed.push(accumulator);
            accumulator = 0;
            filled = 0;
        }
    }
    if filled > 0 {
        packed.push(accumulator << (2 * (4 - filled)));
    }

    let mut payload = Vec::with_capacity(5 + packed.len());
    payload.push(flags);
    payload.extend_from_slice(&count.to_be_bytes());
    payload.extend_from_slice(&packed);
    Ok(payload)
}

fn attribute_pair(name: &str, value: &str) -> String {
    format!(" {}=\"{}\"", name, escape_attribute(value))
}

fn escape_attribute(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn escape_text(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            other => escaped.push(other),
        }
    }
    escaped
}
