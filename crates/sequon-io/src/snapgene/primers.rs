//! Primers XML (packet type 5) mapped to primer_bind features.
//!
//! The empty document shape is corpus-verified; the non-empty Primer element
//! shape is the documented hypothesis F-17 and is therefore parsed
//! defensively: attributes and Segment children are accepted in the documented
//! synthetic shape, unknown fields are kept in extras, and a primer whose
//! binding site cannot be determined is reported and skipped rather than
//! guessed.

use sequon_model::{Feature, JsonValue, Location, Qualifier};

use crate::error::ImportError;

use super::container::Packet;
use super::html;
use super::state::{json_object, ImportState};
use super::xml::{self, XmlNode};

/// Primers parsed from one block.
pub(crate) struct ParsedPrimers {
    /// Canonical primer_bind features in document order.
    pub features: Vec<Feature>,
    /// extras.snapgene.primers[] entries aligned by feature_index.
    pub extras: Vec<JsonValue>,
    /// HybridizationParams settings as ordered name/value pairs.
    pub settings: Vec<JsonValue>,
}

/// Parses a primers XML payload.
pub(crate) fn parse(
    packet: &Packet<'_>,
    sequence_length: u64,
    circular: bool,
    start_index: usize,
    state: &mut ImportState<'_>,
) -> Result<ParsedPrimers, ImportError> {
    let root = xml::parse(packet.payload, state.limits(), packet.offset + 5)
        .map_err(|error| error.into_import_error(packet))?;
    if root.name != "Primers" {
        return Err(ImportError::Malformed {
            code: "DNA_XML_MALFORMED",
            message: "the primers block root element is not Primers".to_string(),
            position: Some(packet.position()),
        });
    }
    let declared = root.children_named("Primer").count() as u64;
    if declared + start_index as u64 > state.limits().max_features {
        return Err(ImportError::LimitExceeded {
            limit: "max_features",
            limit_value: state.limits().max_features,
            actual: declared + start_index as u64,
            position: Some(packet.position()),
        });
    }
    let mut parsed = ParsedPrimers {
        features: Vec::new(),
        extras: Vec::new(),
        settings: Vec::new(),
    };
    for settings in root.children_named("HybridizationParams") {
        for (name, value) in &settings.attributes {
            parsed.settings.push(json_object(vec![
                ("name".to_owned(), JsonValue::from(name.as_str())),
                ("value".to_owned(), JsonValue::from(value.as_str())),
            ]));
        }
    }
    for primer in root.children_named("Primer") {
        let feature_index = start_index + parsed.features.len();
        if feature_index as u64 >= state.limits().max_features {
            return Err(ImportError::LimitExceeded {
                limit: "max_features",
                limit_value: state.limits().max_features,
                actual: feature_index as u64 + 1,
                position: Some(packet.position()),
            });
        }
        if let Some((feature, extras)) = parse_primer(
            primer,
            feature_index,
            sequence_length,
            circular,
            packet,
            state,
        )? {
            parsed.features.push(feature);
            parsed.extras.push(extras);
        }
    }
    Ok(parsed)
}

fn parse_primer(
    node: &XmlNode,
    feature_index: usize,
    sequence_length: u64,
    circular: bool,
    packet: &Packet<'_>,
    state: &mut ImportState<'_>,
) -> Result<Option<(Feature, JsonValue)>, ImportError> {
    let name = primer_name(node);
    let ranges = binding_ranges(node);
    if ranges.is_empty() {
        return skip_primer(
            state,
            packet,
            &name,
            "the primer has no determinable binding site",
        );
    }
    let mut segments = Vec::new();
    for range in &ranges {
        match xml::range_to_segment(range, sequence_length, circular) {
            Ok(segment) => segments.push(segment),
            Err(error) => {
                state.warning(
                    "DNA_FEATURE_RANGE_INVALID",
                    format!("primer binding site cannot be used: {}", error.message()),
                    Some(packet.position()),
                );
                state.drop_feature("coordinate_out_of_range", packet, "primer_bind", range);
                return Ok(None);
            }
        }
    }
    let operator = if segments.len() == 1 {
        sequon_model::OPERATOR_SPAN
    } else {
        sequon_model::OPERATOR_JOIN
    };
    let strand = primer_strand(node);
    let mut qualifiers = Vec::new();
    for qualifier in node.children_named("Q") {
        if let Some(qualifier_name) = qualifier.attribute("name") {
            for value in qualifier.children_named("V") {
                if let Some(text) = value.attribute("text") {
                    qualifiers.push(Qualifier::new(qualifier_name, html::plain_text(text)));
                } else if let Some(integer) = value.attribute("int") {
                    qualifiers.push(Qualifier::new(qualifier_name, integer.trim()));
                }
            }
        }
    }
    let feature = Feature {
        key: "primer_bind".to_string(),
        so_term: crate::so::so_term("primer_bind").map(str::to_string),
        name: name.clone(),
        location: Location {
            operator: operator.to_owned(),
            strand,
            segments,
        },
        qualifiers,
    };
    let extras = primer_extras(feature_index, node, &name, strand);
    Ok(Some((feature, extras)))
}

fn skip_primer(
    state: &mut ImportState<'_>,
    packet: &Packet<'_>,
    name: &Option<String>,
    reason: &str,
) -> Result<Option<(Feature, JsonValue)>, ImportError> {
    let key = name.as_deref().unwrap_or("primer_bind");
    state.warning(
        "DNA_FEATURE_MALFORMED",
        format!("primer entry was skipped: {reason}"),
        Some(packet.position()),
    );
    state.drop_feature("malformed_metadata", packet, key, "");
    Ok(None)
}

fn primer_name(node: &XmlNode) -> Option<String> {
    if let Some(name) = node.attribute("name") {
        if !name.trim().is_empty() {
            return Some(name.to_owned());
        }
    }
    node.child("Name")
        .map(|child| html::plain_text(&child.text))
        .filter(|name| !name.trim().is_empty())
}

fn binding_ranges(node: &XmlNode) -> Vec<String> {
    let mut ranges = node
        .children_named("Segment")
        .filter_map(|segment| segment.attribute("range"))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if ranges.is_empty() {
        for attribute in ["binding_site", "range", "site"] {
            if let Some(value) = node.attribute(attribute) {
                if value.contains('-') {
                    ranges.push(value.to_owned());
                    break;
                }
            }
        }
    }
    if ranges.is_empty() {
        if let (Some(start), Some(end)) = (node.attribute("start"), node.attribute("end")) {
            ranges.push(format!("{start}-{end}"));
        }
    }
    ranges
}

fn primer_strand(node: &XmlNode) -> i8 {
    if let Some(value) = node.attribute("directionality") {
        return match value {
            "2" => -1,
            _ => 1,
        };
    }
    if let Some(value) = node.attribute("strand") {
        return match value.to_ascii_lowercase().as_str() {
            "-1" | "-" | "reverse" => -1,
            "1" | "+" | "forward" => 1,
            _ => 0,
        };
    }
    if let Some(value) = node.attribute("binding") {
        return match value.to_ascii_lowercase().as_str() {
            "reverse" => -1,
            "forward" => 1,
            _ => 0,
        };
    }
    0
}

fn primer_extras(
    feature_index: usize,
    node: &XmlNode,
    name: &Option<String>,
    strand: i8,
) -> JsonValue {
    let attributes = node
        .attributes
        .iter()
        .map(|(name, value)| {
            json_object(vec![
                ("name".to_owned(), JsonValue::from(name.as_str())),
                ("value".to_owned(), JsonValue::from(value.as_str())),
            ])
        })
        .collect::<Vec<_>>();
    let optional = |name: &str| {
        node.attribute(name)
            .map(JsonValue::from)
            .unwrap_or(JsonValue::Null)
    };
    json_object(vec![
        (
            "feature_index".to_owned(),
            JsonValue::from(feature_index as u64),
        ),
        (
            "name".to_owned(),
            name.as_deref()
                .map(JsonValue::from)
                .unwrap_or(JsonValue::Null),
        ),
        ("sequence".to_owned(), optional("sequence")),
        ("binding_sequence".to_owned(), optional("binding_sequence")),
        ("strand".to_owned(), JsonValue::from(strand as i64)),
        ("tm".to_owned(), optional("tm")),
        ("color".to_owned(), optional("color")),
        ("attributes".to_owned(), JsonValue::Array(attributes)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::Limits;

    fn run(xml: &str) -> (ParsedPrimers, ImportState<'static>) {
        let limits: &'static Limits = Box::leak(Box::new(Limits::default()));
        let mut state = ImportState::new(limits, true);
        let payload = xml.as_bytes();
        let owner = Packet {
            index: 1,
            packet_type: 5,
            offset: 100,
            payload,
        };
        let parsed = parse(&owner, 100, false, 0, &mut state).expect("parse primers");
        (parsed, state)
    }

    #[test]
    fn parses_empty_primers_with_settings() {
        let xml = r#"<Primers nextValidID="0"><HybridizationParams minMeltingTemperature="40"/></Primers>"#;
        let (parsed, state) = run(xml);
        assert!(parsed.features.is_empty());
        assert_eq!(parsed.settings.len(), 1);
        assert!(state.diagnostics.is_empty());
    }

    #[test]
    fn parses_hypothesis_primer_shape() {
        let xml = r#"<Primers nextValidID="1">
            <Primer name="M13F" sequence="GTAAAACGACGGCCAGT" binding_site="1-17" directionality="1" tm="58.4"/>
          </Primers>"#;
        let (parsed, state) = run(xml);
        assert_eq!(parsed.features.len(), 1);
        assert_eq!(parsed.features[0].key, "primer_bind");
        assert_eq!(parsed.features[0].name.as_deref(), Some("M13F"));
        assert_eq!(parsed.features[0].location.strand, 1);
        assert_eq!(parsed.features[0].location.segments[0].start, 0);
        assert_eq!(parsed.features[0].location.segments[0].end, 17);
        assert!(state.diagnostics.is_empty());
    }

    #[test]
    fn skips_primer_without_binding_site() {
        let xml = r#"<Primers><Primer name="lost"/></Primers>"#;
        let (parsed, state) = run(xml);
        assert!(parsed.features.is_empty());
        assert_eq!(state.dropped[0].reason, "malformed_metadata");
        assert_eq!(state.diagnostics[0].code, "DNA_FEATURE_MALFORMED");
    }
}
