//! Notes XML (packet type 6): ordered metadata fields and name sources.
//!
//! The field names and order are corpus-verified (design note section 6.5).
//! Every field is preserved verbatim in extras.snapgene.notes; the canonical
//! name and description use the plain-text form of the relevant fields.

use sequon_model::JsonValue;

use crate::error::ImportError;

use super::container::Packet;
use super::html;
use super::state::{json_object, ImportState};
use super::xml;

/// Parsed notes block.
pub(crate) struct NotesData {
    /// extras.snapgene.notes payload.
    pub extras: JsonValue,
    /// Plain text of AccessionNumber, when present and non-empty.
    pub accession: Option<String>,
    /// Plain text of CustomMapLabel, when present and non-empty.
    pub custom_map_label: Option<String>,
    /// Whether UseCustomMapLabel enables the custom map label.
    pub use_custom_map_label: bool,
    /// Plain text of Description, when present and non-empty.
    pub description: Option<String>,
    /// Plain text of Comments, when present and non-empty.
    pub comments: Option<String>,
}

/// Parses a notes XML payload.
pub(crate) fn parse(
    packet: &Packet<'_>,
    state: &ImportState<'_>,
) -> Result<NotesData, ImportError> {
    let root = xml::parse(packet.payload, state.limits(), packet.offset + 5)
        .map_err(|error| error.into_import_error(packet))?;
    if root.name != "Notes" {
        return Err(ImportError::Malformed {
            code: "DNA_XML_MALFORMED",
            message: "the notes block root element is not Notes".to_string(),
            position: Some(packet.position()),
        });
    }
    let mut fields = Vec::new();
    let mut parsed = NotesData {
        extras: JsonValue::Array(Vec::new()),
        accession: None,
        custom_map_label: None,
        use_custom_map_label: false,
        description: None,
        comments: None,
    };
    for field in &root.children {
        let raw = field.text.as_str();
        let plain = html::plain_text(raw);
        let trimmed = plain.trim().to_owned();
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
            ("value".to_owned(), JsonValue::from(raw)),
            ("attributes".to_owned(), JsonValue::Array(attributes)),
        ]));
        if trimmed.is_empty() {
            continue;
        }
        match field.name.as_str() {
            "AccessionNumber" => parsed.accession = Some(trimmed),
            "CustomMapLabel" => parsed.custom_map_label = Some(trimmed),
            "UseCustomMapLabel" => {
                parsed.use_custom_map_label =
                    matches!(trimmed.to_ascii_lowercase().as_str(), "1" | "true" | "yes")
            }
            "Description" => parsed.description = Some(trimmed),
            "Comments" => parsed.comments = Some(trimmed),
            _ => {}
        }
    }
    parsed.extras = JsonValue::Array(fields);
    Ok(parsed)
}
