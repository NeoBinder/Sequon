//! Sequence properties XML (packet type 8).
//!
//! The corpus block holds ligation/ends settings, not sequence facts (design
//! note section 6.5). Everything is preserved in extras.snapgene.properties; a
//! topology-looking field is used only to raise DNA_TOPOLOGY_CONFLICT, never to
//! override the DNA flags.

use sequon_model::{JsonValue, Topology};

use crate::error::ImportError;

use super::container::Packet;
use super::state::{json_object, ImportState};
use super::xml;

/// Parsed properties block.
pub(crate) struct PropertiesData {
    /// extras.snapgene.properties payload.
    pub extras: JsonValue,
    /// Topology-looking value from the block, if one is recognized.
    pub topology: Option<Topology>,
}

/// Parses an AdditionalSequenceProperties payload.
pub(crate) fn parse(
    packet: &Packet<'_>,
    state: &ImportState<'_>,
) -> Result<PropertiesData, ImportError> {
    let root = xml::parse(packet.payload, state.limits(), packet.offset + 5)
        .map_err(|error| error.into_import_error(packet))?;
    if root.name != "AdditionalSequenceProperties" {
        return Err(ImportError::Malformed {
            code: "DNA_XML_MALFORMED",
            message: "the properties block root element is not AdditionalSequenceProperties"
                .to_string(),
            position: Some(packet.position()),
        });
    }
    let mut fields = Vec::new();
    let mut topology = None;
    for field in &root.children {
        let attributes = field
            .attributes
            .iter()
            .map(|(name, value)| {
                json_object(vec![
                    ("name".to_owned(), JsonValue::from(name.as_str())),
                    ("value".to_owned(), JsonValue::from(value.as_str())),
                ])
            })
            .collect::<Vec<_>>();
        fields.push(json_object(vec![
            ("name".to_owned(), JsonValue::from(field.name.as_str())),
            ("value".to_owned(), JsonValue::from(field.text.as_str())),
            ("attributes".to_owned(), JsonValue::Array(attributes)),
        ]));
        if topology.is_none() {
            topology = topology_from_field(&field.name, field.text.trim());
        }
    }
    Ok(PropertiesData {
        extras: JsonValue::Array(fields),
        topology,
    })
}

fn topology_from_field(name: &str, value: &str) -> Option<Topology> {
    let truthy = matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes");
    let falsey = matches!(value.to_ascii_lowercase().as_str(), "0" | "false" | "no");
    match name {
        "IsCircular" | "Circular" if truthy => Some(Topology::Circular),
        "IsCircular" | "Circular" if falsey => Some(Topology::Linear),
        "IsLinear" | "Linear" if truthy => Some(Topology::Linear),
        "IsLinear" | "Linear" if falsey => Some(Topology::Circular),
        _ => None,
    }
}
