//! Safe XML tokenizer and a small event-built tree for SnapGene XML blocks.
//!
//! Every XML block is parsed with the same envelope: UTF-8 only, no DTD, only
//! the five predefined entities plus numeric character references, a depth cap
//! and a per-element text cap. Byte offsets in errors are absolute file offsets
//! (packet payload base plus the tokenizer offset).
//!
//! The tokenizer is quick-xml 0.42.0 with default features off (MIT); the
//! SnapGene element semantics live in this crate, not in the tokenizer.

use quick_xml::events::{BytesRef, BytesStart, Event};
use quick_xml::reader::Reader;
use quick_xml::XmlVersion;

use crate::error::ImportError;
use crate::limits::Limits;
use crate::report::SourcePosition;
use sequon_model::Segment;

use super::container::Packet;

/// One parsed XML element.
#[derive(Debug, Clone, Default)]
pub(crate) struct XmlNode {
    /// Local element name as written.
    pub name: String,
    /// Attributes in source order, values XML-unescaped.
    pub attributes: Vec<(String, String)>,
    /// Child elements in source order.
    pub children: Vec<XmlNode>,
    /// Accumulated text content (text plus resolved references plus CDATA).
    pub text: String,
}

impl XmlNode {
    /// Returns one attribute value by name.
    pub(crate) fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// Returns the first child with the given name.
    pub(crate) fn child(&self, name: &str) -> Option<&XmlNode> {
        self.children.iter().find(|child| child.name == name)
    }

    /// Iterates children with the given name in source order.
    pub(crate) fn children_named<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Iterator<Item = &'a XmlNode> + 'a {
        self.children.iter().filter(move |child| child.name == name)
    }
}

/// Error returned when a `range="A-B"` attribute cannot be converted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RangeError {
    message: String,
}

impl RangeError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the human-readable reason.
    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}

/// Converts a SnapGene 1-based closed `range="A-B"` into one canonical segment.
///
/// The convention is corpus-verified (design note section 6.3): `start0 = A - 1`
/// and `end0 = B`, on the recorded forward strand. A range that crosses the
/// origin of a circular sequence stays one unwrapped segment with `end > length`
/// (the 2026-10-23 model revision); `Segment::resolved` maps it back on access.
/// A circular range that spans more than one full turn, or any out-of-range
/// linear range, is an error.
pub(crate) fn range_to_segment(
    range: &str,
    length: u64,
    circular: bool,
) -> Result<Segment, RangeError> {
    let mut parts = range.split('-');
    let start_text = parts.next().unwrap_or("");
    let end_text = parts.next().unwrap_or("");
    if parts.next().is_some() || start_text.is_empty() || end_text.is_empty() {
        return Err(RangeError::new("range must have the form A-B"));
    }
    let start = start_text
        .parse::<u64>()
        .map_err(|_| RangeError::new("range start is not a decimal integer"))?;
    let end = end_text
        .parse::<u64>()
        .map_err(|_| RangeError::new("range end is not a decimal integer"))?;
    if start == 0 {
        return Err(RangeError::new(
            "range start is 1-based and must be at least 1",
        ));
    }
    if end < start {
        return Err(RangeError::new("range end is smaller than its start"));
    }
    if start > length {
        return Err(RangeError::new("range start is beyond the sequence length"));
    }
    let start0 = start - 1;
    let end0 = end;
    if end0 <= length {
        return Ok(Segment::new(start0, end0));
    }
    if !circular {
        return Err(RangeError::new(
            "range end is beyond the length of a linear sequence",
        ));
    }
    if end0 - start0 > length {
        return Err(RangeError::new(
            "range spans more than one full turn of a circular sequence",
        ));
    }
    // Keep the source range unwrapped instead of splitting it at the origin:
    // one segment may end past `length` by at most its start (one wrap).
    Ok(Segment::new(start0, end0))
}

/// Error produced by the safe XML envelope.
#[derive(Debug)]
pub(crate) struct XmlError {
    code: &'static str,
    limit: Option<(&'static str, u64, u64)>,
    offset: u64,
    message: String,
}

impl XmlError {
    fn malformed(offset: u64, message: impl Into<String>) -> Self {
        Self {
            code: "DNA_XML_MALFORMED",
            limit: None,
            offset,
            message: message.into(),
        }
    }

    fn depth(offset: u64, limit: u64) -> Self {
        Self {
            code: "DNA_XML_DEPTH_EXCEEDED",
            limit: None,
            offset,
            message: format!("XML nesting exceeds the configured depth limit {limit}"),
        }
    }

    fn nodes(offset: u64, limit: u64, actual: u64) -> Self {
        Self {
            code: "DNA_XML_MALFORMED",
            limit: Some(("max_xml_nodes", limit, actual)),
            offset,
            message: "XML node count exceeds the configured limit".to_string(),
        }
    }

    fn element_size(offset: u64, limit: u64, actual: u64) -> Self {
        Self {
            code: "DNA_XML_MALFORMED",
            limit: Some(("max_xml_element_bytes", limit, actual)),
            offset,
            message: "XML element text exceeds the configured size limit".to_string(),
        }
    }

    /// Returns the diagnostic code this error maps to.
    pub(crate) fn code(&self) -> &'static str {
        self.code
    }

    /// Returns the human-readable explanation.
    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    /// Converts the error into the typed import error with a packet position.
    pub(crate) fn into_import_error(self, packet: &Packet<'_>) -> ImportError {
        let position = Some(SourcePosition::Snapgene {
            offset: self.offset,
            packet_type: Some(u32::from(packet.packet_type)),
            packet_index: Some(packet.index),
        });
        match self.limit {
            Some((limit, limit_value, actual)) => ImportError::LimitExceeded {
                limit,
                limit_value,
                actual,
                position,
            },
            None => ImportError::Malformed {
                code: self.code,
                message: self.message,
                position,
            },
        }
    }
}

#[derive(Debug)]
enum XmlEvent {
    Start {
        name: String,
        attributes: Vec<(String, String)>,
    },
    Empty {
        name: String,
        attributes: Vec<(String, String)>,
    },
    End,
    Text(String),
}

/// Parses an XML payload into a tree.
pub(crate) fn parse(
    payload: &[u8],
    limits: &Limits,
    base_offset: u64,
) -> Result<XmlNode, XmlError> {
    let input = utf8(payload, base_offset)?;
    let mut stack: Vec<XmlNode> = Vec::new();
    let mut root: Option<XmlNode> = None;
    run(input, limits, base_offset, |event| {
        match event {
            XmlEvent::Start { name, attributes } => {
                stack.push(XmlNode {
                    name,
                    attributes,
                    children: Vec::new(),
                    text: String::new(),
                });
            }
            XmlEvent::Empty { name, attributes } => {
                let node = XmlNode {
                    name,
                    attributes,
                    children: Vec::new(),
                    text: String::new(),
                };
                attach(node, &mut stack, &mut root);
            }
            XmlEvent::End => {
                if let Some(node) = stack.pop() {
                    attach(node, &mut stack, &mut root);
                }
            }
            XmlEvent::Text(text) => {
                if let Some(current) = stack.last_mut() {
                    current.text.push_str(&text);
                }
            }
        }
        Ok(())
    })?;
    root.ok_or_else(|| XmlError::malformed(base_offset, "XML has no root element"))
}

/// Validates an XML payload without building a tree.
///
/// Used for history blocks: the content is checked for well-formedness and the
/// same safety envelope, but it is never mapped into the model.
pub(crate) fn validate(payload: &[u8], limits: &Limits, base_offset: u64) -> Result<(), XmlError> {
    let input = utf8(payload, base_offset)?;
    run(input, limits, base_offset, |_event| Ok(()))
}

fn utf8(payload: &[u8], base_offset: u64) -> Result<&str, XmlError> {
    std::str::from_utf8(payload).map_err(|error| {
        XmlError::malformed(
            base_offset + error.valid_up_to() as u64,
            "the XML block is not valid UTF-8",
        )
    })
}

fn attach(node: XmlNode, stack: &mut [XmlNode], root: &mut Option<XmlNode>) {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else if root.is_none() {
        *root = Some(node);
    }
}

fn run<F>(input: &str, limits: &Limits, base_offset: u64, mut emit: F) -> Result<(), XmlError>
where
    F: FnMut(XmlEvent) -> Result<(), XmlError>,
{
    let max_depth = usize::try_from(limits.max_xml_depth.max(1)).unwrap_or(usize::MAX);
    let max_element = limits.max_xml_element_bytes;
    let mut reader = Reader::from_str(input);
    {
        let config = reader.config_mut();
        config.check_end_names = true;
        config.expand_empty_elements = false;
        config.allow_dangling_amp = false;
    }
    let mut depth = 0_usize;
    let mut root_seen = false;
    let mut node_count: u64 = 0;
    let mut text_bytes: Vec<u64> = Vec::new();
    loop {
        let event_start = reader.buffer_position();
        let event = match reader.read_event() {
            Ok(event) => event,
            Err(error) => {
                return Err(XmlError::malformed(
                    base_offset + reader.error_position(),
                    format!("XML is not well formed: {error}"),
                ))
            }
        };
        match event {
            Event::Start(element) => {
                if depth == 0 && root_seen {
                    return Err(XmlError::malformed(
                        base_offset + event_start,
                        "XML has more than one root element",
                    ));
                }
                depth += 1;
                if depth > max_depth {
                    return Err(XmlError::depth(
                        base_offset + event_start,
                        limits.max_xml_depth,
                    ));
                }
                let attributes_count = count_attributes(&element, base_offset, event_start)?;
                node_count = node_count
                    .saturating_add(1)
                    .saturating_add(attributes_count);
                if node_count > limits.max_xml_nodes {
                    return Err(XmlError::nodes(
                        base_offset + event_start,
                        limits.max_xml_nodes,
                        node_count,
                    ));
                }
                let name = element.name().as_ref().to_owned();
                let attributes = read_attributes(&element, base_offset, event_start)?;
                emit(XmlEvent::Start { name, attributes })?;
                text_bytes.push(0);
                root_seen = true;
            }
            Event::Empty(element) => {
                if depth == 0 {
                    if root_seen {
                        return Err(XmlError::malformed(
                            base_offset + event_start,
                            "XML has more than one root element",
                        ));
                    }
                    root_seen = true;
                }
                if depth + 1 > max_depth {
                    return Err(XmlError::depth(
                        base_offset + event_start,
                        limits.max_xml_depth,
                    ));
                }
                let attributes_count = count_attributes(&element, base_offset, event_start)?;
                node_count = node_count
                    .saturating_add(1)
                    .saturating_add(attributes_count);
                if node_count > limits.max_xml_nodes {
                    return Err(XmlError::nodes(
                        base_offset + event_start,
                        limits.max_xml_nodes,
                        node_count,
                    ));
                }
                let name = element.name().as_ref().to_owned();
                let attributes = read_attributes(&element, base_offset, event_start)?;
                emit(XmlEvent::Empty { name, attributes })?;
            }
            Event::End(_) => {
                if depth == 0 {
                    return Err(XmlError::malformed(
                        base_offset + event_start,
                        "XML has a closing tag without an opening tag",
                    ));
                }
                depth -= 1;
                let _ = text_bytes.pop();
                emit(XmlEvent::End)?;
            }
            Event::Text(text) => {
                let content = normalize_eols(text.as_ref());
                text_event(
                    content.as_ref(),
                    depth,
                    &mut text_bytes,
                    max_element,
                    base_offset + event_start,
                    &mut emit,
                )?;
            }
            Event::CData(cdata) => {
                let content = normalize_eols(cdata.as_ref());
                text_event(
                    content.as_ref(),
                    depth,
                    &mut text_bytes,
                    max_element,
                    base_offset + event_start,
                    &mut emit,
                )?;
            }
            Event::GeneralRef(reference) => {
                let resolved = resolve_reference(&reference, base_offset, event_start)?;
                text_event(
                    &resolved,
                    depth,
                    &mut text_bytes,
                    max_element,
                    base_offset + event_start,
                    &mut emit,
                )?;
            }
            Event::DocType(_) => {
                return Err(XmlError::malformed(
                    base_offset + event_start,
                    "a document type declaration is not allowed",
                ))
            }
            Event::Decl(_) | Event::PI(_) | Event::Comment(_) => {}
            Event::Eof => {
                if depth != 0 {
                    return Err(XmlError::malformed(
                        base_offset + event_start,
                        "XML has an unclosed element",
                    ));
                }
                if !root_seen {
                    return Err(XmlError::malformed(
                        base_offset + event_start,
                        "XML has no root element",
                    ));
                }
                break;
            }
        }
    }
    Ok(())
}

fn text_event<F>(
    text: &str,
    depth: usize,
    text_bytes: &mut [u64],
    max_element: u64,
    offset: u64,
    emit: &mut F,
) -> Result<(), XmlError>
where
    F: FnMut(XmlEvent) -> Result<(), XmlError>,
{
    if depth == 0 {
        if text.trim().is_empty() {
            return Ok(());
        }
        return Err(XmlError::malformed(
            offset,
            "text appears outside the root element",
        ));
    }
    if let Some(total) = text_bytes.last_mut() {
        *total = total.saturating_add(text.len() as u64);
        if *total > max_element {
            return Err(XmlError::element_size(offset, max_element, *total));
        }
    }
    emit(XmlEvent::Text(text.to_owned()))
}

fn count_attributes(
    element: &BytesStart<'_>,
    base_offset: u64,
    event_start: u64,
) -> Result<u64, XmlError> {
    let mut count = 0_u64;
    for attribute in element.attributes() {
        attribute.map_err(|error| {
            XmlError::malformed(
                base_offset + event_start,
                format!("XML attribute is not well formed: {error}"),
            )
        })?;
        count = count.saturating_add(1);
    }
    Ok(count)
}

fn read_attributes(
    element: &BytesStart<'_>,
    base_offset: u64,
    event_start: u64,
) -> Result<Vec<(String, String)>, XmlError> {
    let mut attributes: Vec<(String, String)> = Vec::new();
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|error| {
            XmlError::malformed(
                base_offset + event_start,
                format!("XML attribute is not well formed: {error}"),
            )
        })?;
        let key = attribute.key.as_ref().to_owned();
        let value = attribute
            .normalized_value(XmlVersion::Implicit1_0)
            .map_err(|error| {
                XmlError::malformed(
                    base_offset + event_start,
                    format!("XML attribute value cannot be decoded: {error}"),
                )
            })?
            .into_owned();
        if attributes.iter().any(|(existing, _)| *existing == key) {
            return Err(XmlError::malformed(
                base_offset + event_start,
                "XML has a duplicated attribute",
            ));
        }
        attributes.push((key, value));
    }
    Ok(attributes)
}

fn resolve_reference(
    reference: &BytesRef<'_>,
    base_offset: u64,
    event_start: u64,
) -> Result<String, XmlError> {
    if let Some(character) = reference.resolve_char_ref().map_err(|error| {
        XmlError::malformed(
            base_offset + event_start,
            format!("XML character reference is invalid: {error}"),
        )
    })? {
        return Ok(character.to_string());
    }
    let decoded = match reference.as_ref() {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "apos" => "'",
        "quot" => "\"",
        other => {
            return Err(XmlError::malformed(
                base_offset + event_start,
                format!("entity reference {other:?} is not one of the predefined XML entities"),
            ))
        }
    };
    Ok(decoded.to_owned())
}

fn normalize_eols(input: &str) -> std::borrow::Cow<'_, str> {
    if input.contains('\r') {
        std::borrow::Cow::Owned(input.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        std::borrow::Cow::Borrowed(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(payload: &[u8]) -> Packet<'_> {
        Packet {
            index: 1,
            packet_type: 10,
            offset: 100,
            payload,
        }
    }

    fn limits() -> Limits {
        Limits::default()
    }

    #[test]
    fn parses_attributes_entities_and_cdata() {
        let xml =
            br#"<root a="1&amp;2"><child>text &lt;x&gt; &#65;</child><c><![CDATA[cd]]></c></root>"#;
        let node = parse(xml, &limits(), 100).expect("parse");
        assert_eq!(node.name, "root");
        assert_eq!(node.attribute("a"), Some("1&2"));
        assert_eq!(node.children.len(), 2);
        assert_eq!(node.children[0].text, "text <x> A");
        assert_eq!(node.children[1].text, "cd");
    }

    #[test]
    fn rejects_dtd() {
        let xml = br#"<!DOCTYPE root><root/>"#;
        let error = parse(xml, &limits(), 0).expect_err("dtd");
        assert_eq!(error.code, "DNA_XML_MALFORMED");
    }

    #[test]
    fn rejects_external_entity_reference() {
        let xml = br#"<root>&external;</root>"#;
        let error = parse(xml, &limits(), 0).expect_err("entity");
        assert_eq!(error.code, "DNA_XML_MALFORMED");
    }

    #[test]
    fn enforces_node_limit_before_building_nodes() {
        let limits = Limits {
            max_xml_nodes: 3,
            ..Limits::default()
        };
        // root (1) + two attributes (2) + child (1) = 4 nodes.
        let xml = br#"<a x="1" y="2"><b/></a>"#;
        let error = parse(xml, &limits, 0).expect_err("nodes");
        assert_eq!(error.limit, Some(("max_xml_nodes", 3, 4)));

        let limits = Limits {
            max_xml_nodes: 4,
            ..Limits::default()
        };
        assert!(parse(xml, &limits, 0).is_ok());
    }

    #[test]
    fn enforces_depth_limit() {
        let mut limits = limits();
        limits.max_xml_depth = 2;
        let xml = b"<a><b><c/></b></a>";
        let error = parse(xml, &limits, 0).expect_err("depth");
        assert_eq!(error.code, "DNA_XML_DEPTH_EXCEEDED");
    }

    #[test]
    fn enforces_element_size_limit() {
        let mut limits = limits();
        limits.max_xml_element_bytes = 4;
        let xml = b"<a>12345</a>";
        let error = parse(xml, &limits, 0).expect_err("size");
        assert!(matches!(error.limit, Some(("max_xml_element_bytes", 4, 5))));
    }

    #[test]
    fn reports_absolute_error_offset() {
        let xml = b"<a><b></a></b>";
        let error = parse(xml, &limits(), 1000).expect_err("malformed");
        assert_eq!(error.code, "DNA_XML_MALFORMED");
        assert!(error.offset >= 1000);
    }

    #[test]
    fn validate_accepts_well_formed_input_and_rejects_malformed() {
        assert!(validate(b"<a><b/></a>", &limits(), 0).is_ok());
        assert!(validate(b"<a>", &limits(), 0).is_err());
    }

    #[test]
    fn range_conversion_matches_the_corpus_examples() {
        // MCS 158-203 on a 5369 bp circular sequence.
        assert_eq!(
            range_to_segment("158-203", 5369, true).expect("range"),
            Segment::new(157, 203)
        );
        // T7 promoter 368-386.
        assert_eq!(
            range_to_segment("368-386", 5369, true).expect("range"),
            Segment::new(367, 386)
        );
        // A single base at the last position.
        assert_eq!(
            range_to_segment("5369-5369", 5369, true).expect("range"),
            Segment::new(5368, 5369)
        );
    }

    #[test]
    fn range_conversion_keeps_a_circular_wrap_unwrapped() {
        // 5368-5372 crosses the origin of a 5369 bp circle: one segment with
        // end > length, not two origin-split pieces.
        let segment = range_to_segment("5368-5372", 5369, true).expect("range");
        assert_eq!(segment, Segment::new(5367, 5372));
        assert!(segment.is_wrapped(5369));
        assert_eq!(
            segment.resolved(5369),
            vec![Segment::new(5367, 5369), Segment::new(0, 3)]
        );

        // A full turn starting at base 2 has span == length, the largest legal
        // circular span, and stays one unwrapped segment.
        let segment = range_to_segment("2-5370", 5369, true).expect("range");
        assert_eq!(segment, Segment::new(1, 5370));
        assert_eq!(segment.resolved(5369).len(), 2);
    }

    #[test]
    fn range_conversion_rejects_bad_input() {
        assert!(range_to_segment("0-10", 100, false).is_err());
        assert!(range_to_segment("20-10", 100, false).is_err());
        assert!(range_to_segment("10-101", 100, false).is_err());
        assert!(range_to_segment("101-101", 100, true).is_err());
        // A two-turn span is not expressible as one wrapped segment.
        assert!(range_to_segment("1-200", 100, true).is_err());
        assert!(range_to_segment("1-201", 100, true).is_err());
        assert!(range_to_segment("1-2-3", 100, false).is_err());
        assert!(range_to_segment("a-b", 100, false).is_err());
    }

    #[test]
    fn into_import_error_uses_packet_fields() {
        let payload = b"<a>";
        let owner = packet(payload);
        let error = parse(payload, &limits(), 100).expect_err("malformed");
        let import_error = error.into_import_error(&owner);
        assert_eq!(import_error.code(), "DNA_XML_MALFORMED");
        match import_error.position() {
            Some(SourcePosition::Snapgene {
                offset,
                packet_type,
                packet_index,
            }) => {
                assert!(*offset >= 100);
                assert_eq!(*packet_type, Some(10));
                assert_eq!(*packet_index, Some(1));
            }
            other => panic!("unexpected position: {other:?}"),
        }
    }
}
