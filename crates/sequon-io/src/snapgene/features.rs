//! Features XML (packet type 10) mapped to canonical features.
//!
//! The element and attribute shapes are corpus-verified (design note section
//! 6.2): one Feature per element in document order, a type attribute for the
//! key, an optional name attribute, an optional directionality attribute
//! (absent or 1 forward, 2 reverse), one or more Segment range attributes, and
//! Q children whose value is the V child's text or int attribute.
//!
//! Text values are HTML documents; canonical qualifier values carry the
//! plain-text form and the raw V value is kept in extras (lead decision).

use sequon_model::{Feature, JsonValue, Location, Qualifier};

use crate::error::ImportError;

use super::container::Packet;
use super::html;
use super::state::{json_object, ImportState};
use super::xml::{self, XmlNode};

/// Features parsed from one block.
pub(crate) struct ParsedFeatures {
    /// Canonical features in document order.
    pub features: Vec<Feature>,
    /// extras.snapgene.features[] entries aligned by feature_index.
    pub extras: Vec<JsonValue>,
}

/// Parses a features XML payload.
pub(crate) fn parse(
    packet: &Packet<'_>,
    sequence_length: u64,
    circular: bool,
    start_index: usize,
    state: &mut ImportState<'_>,
) -> Result<ParsedFeatures, ImportError> {
    let root = xml::parse(packet.payload, state.limits(), packet.offset + 5)
        .map_err(|error| error.into_import_error(packet))?;
    if root.name != "Features" {
        return Err(ImportError::Malformed {
            code: "DNA_XML_MALFORMED",
            message: "the features block root element is not Features".to_string(),
            position: Some(packet.position()),
        });
    }
    let declared = root.children_named("Feature").count() as u64;
    if declared + start_index as u64 > state.limits().max_features {
        return Err(ImportError::LimitExceeded {
            limit: "max_features",
            limit_value: state.limits().max_features,
            actual: declared + start_index as u64,
            position: Some(packet.position()),
        });
    }
    let mut parsed = ParsedFeatures {
        features: Vec::new(),
        extras: Vec::new(),
    };
    for feature_node in root.children_named("Feature") {
        let feature_index = start_index + parsed.features.len();
        if feature_index as u64 >= state.limits().max_features {
            return Err(ImportError::LimitExceeded {
                limit: "max_features",
                limit_value: state.limits().max_features,
                actual: feature_index as u64 + 1,
                position: Some(packet.position()),
            });
        }
        if let Some((feature, extras)) = parse_feature(
            feature_node,
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

fn parse_feature(
    node: &XmlNode,
    feature_index: usize,
    sequence_length: u64,
    circular: bool,
    packet: &Packet<'_>,
    state: &mut ImportState<'_>,
) -> Result<Option<(Feature, JsonValue)>, ImportError> {
    let key = node.attribute("type").unwrap_or("").trim().to_owned();
    let location_text = node
        .children_named("Segment")
        .filter_map(|segment| segment.attribute("range"))
        .collect::<Vec<_>>()
        .join(",");
    if key.is_empty() {
        return handle_feature_skip(
            state,
            packet,
            "",
            &location_text,
            "feature entry has no usable type attribute",
        );
    }
    let directionality = match node.attribute("directionality") {
        None => 1_i8,
        Some("1") => 1,
        Some("2") => -1,
        Some(_) => {
            return handle_feature_skip(
                state,
                packet,
                &key,
                &location_text,
                "feature entry has an unsupported directionality value",
            )
        }
    };
    let mut segments = Vec::new();
    for segment in node.children_named("Segment") {
        let range = match segment.attribute("range") {
            Some(range) => range,
            None => {
                return handle_feature_skip(
                    state,
                    packet,
                    &key,
                    &location_text,
                    "feature segment has no range attribute",
                )
            }
        };
        match xml::range_to_segment(range, sequence_length, circular) {
            Ok(segment) => segments.push(segment),
            Err(error) => {
                state.warning(
                    "DNA_FEATURE_RANGE_INVALID",
                    format!("feature range cannot be used: {}", error.message()),
                    Some(packet.position()),
                );
                state.drop_feature("coordinate_out_of_range", packet, &key, &location_text);
                return Ok(None);
            }
        }
        if segments.len() as u64 > state.limits().max_segments_per_feature {
            return Err(ImportError::LimitExceeded {
                limit: "max_segments_per_feature",
                limit_value: state.limits().max_segments_per_feature,
                actual: segments.len() as u64,
                position: Some(packet.position()),
            });
        }
    }
    if segments.is_empty() {
        return handle_feature_skip(
            state,
            packet,
            &key,
            &location_text,
            "feature entry has no usable segment",
        );
    }
    let operator = if segments.len() == 1 {
        sequon_model::OPERATOR_SPAN
    } else {
        sequon_model::OPERATOR_JOIN
    };
    let location = Location {
        operator: operator.to_owned(),
        strand: directionality,
        segments,
    };
    let name = node
        .attribute("name")
        .filter(|name| !name.trim().is_empty())
        .map(|name| name.to_owned());
    let mut qualifiers = Vec::new();
    let mut raw_qualifiers = Vec::new();
    for qualifier in node.children_named("Q") {
        let qualifier_name = match qualifier.attribute("name") {
            Some(name) if !name.trim().is_empty() => name.to_owned(),
            _ => {
                return handle_feature_skip(
                    state,
                    packet,
                    &key,
                    &location_text,
                    "feature qualifier has no usable name",
                )
            }
        };
        let mut any_value = false;
        for value in qualifier.children_named("V") {
            let (canonical, raw, value_type) = if let Some(text) = value.attribute("text") {
                let canonical = html::plain_text(text);
                check_qualifier_size(state, text.len() as u64, packet)?;
                check_qualifier_size(state, canonical.len() as u64, packet)?;
                (canonical, text.to_owned(), "text")
            } else if let Some(integer) = value.attribute("int") {
                let integer = integer.trim().to_owned();
                check_qualifier_size(state, integer.len() as u64, packet)?;
                (integer.clone(), integer, "int")
            } else {
                return handle_feature_skip(
                    state,
                    packet,
                    &key,
                    &location_text,
                    "feature qualifier value has neither text nor int",
                );
            };
            any_value = true;
            qualifiers.push(Qualifier::new(qualifier_name.clone(), canonical));
            raw_qualifiers.push(json_object(vec![
                ("value_type".to_owned(), JsonValue::from(value_type)),
                ("raw".to_owned(), JsonValue::from(raw)),
            ]));
        }
        if !any_value {
            qualifiers.push(Qualifier {
                key: qualifier_name,
                value: None,
            });
        }
    }
    let extras = feature_extras(feature_index, &key, node, &raw_qualifiers);
    let so_term = crate::so::so_term(&key).map(str::to_string);
    Ok(Some((
        Feature {
            key,
            so_term,
            name,
            location,
            qualifiers,
        },
        extras,
    )))
}

fn handle_feature_skip(
    state: &mut ImportState<'_>,
    packet: &Packet<'_>,
    feature_key: &str,
    location_text: &str,
    reason: &str,
) -> Result<Option<(Feature, JsonValue)>, ImportError> {
    state.warning(
        "DNA_FEATURE_MALFORMED",
        format!("feature entry was skipped: {reason}"),
        Some(packet.position()),
    );
    state.drop_feature("malformed_metadata", packet, feature_key, location_text);
    Ok(None)
}

fn check_qualifier_size(
    state: &ImportState<'_>,
    actual: u64,
    packet: &Packet<'_>,
) -> Result<(), ImportError> {
    let limit = state.limits().max_qualifier_value_bytes;
    if actual > limit {
        return Err(ImportError::LimitExceeded {
            limit: "max_qualifier_value_bytes",
            limit_value: limit,
            actual,
            position: Some(packet.position()),
        });
    }
    Ok(())
}

fn feature_extras(
    feature_index: usize,
    key: &str,
    node: &XmlNode,
    qualifiers: &[JsonValue],
) -> JsonValue {
    let attributes = node
        .attributes
        .iter()
        .filter(|(name, _)| name != "name" && name != "type" && name != "directionality")
        .map(|(name, value)| {
            json_object(vec![
                ("name".to_owned(), JsonValue::from(name.as_str())),
                ("value".to_owned(), JsonValue::from(value.as_str())),
            ])
        })
        .collect::<Vec<_>>();
    let segments = node
        .children_named("Segment")
        .enumerate()
        .map(|(segment_index, segment)| {
            let attributes = segment
                .attributes
                .iter()
                .filter(|(name, _)| name != "range")
                .map(|(name, value)| {
                    json_object(vec![
                        ("name".to_owned(), JsonValue::from(name.as_str())),
                        ("value".to_owned(), JsonValue::from(value.as_str())),
                    ])
                })
                .collect::<Vec<_>>();
            json_object(vec![
                (
                    "segment_index".to_owned(),
                    JsonValue::from(segment_index as u64),
                ),
                ("attributes".to_owned(), JsonValue::Array(attributes)),
            ])
        })
        .collect::<Vec<_>>();
    json_object(vec![
        (
            "feature_index".to_owned(),
            JsonValue::from(feature_index as u64),
        ),
        ("key".to_owned(), JsonValue::from(key)),
        ("attributes".to_owned(), JsonValue::Array(attributes)),
        ("segments".to_owned(), JsonValue::Array(segments)),
        (
            "qualifiers".to_owned(),
            JsonValue::Array(qualifiers.to_vec()),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::Limits;

    fn packet(payload: &[u8], index: u32) -> Packet<'_> {
        Packet {
            index,
            packet_type: 10,
            offset: 100,
            payload,
        }
    }

    fn parse_features(
        xml: &str,
        length: u64,
        circular: bool,
    ) -> (ParsedFeatures, ImportState<'static>) {
        // The state borrows the limits; leak one for the test to keep the
        // signature simple without unsafe code.
        let limits: &'static Limits = Box::leak(Box::new(Limits::default()));
        let mut state = ImportState::new(limits, true);
        let payload = xml.as_bytes();
        let owner = packet(payload, 1);
        let parsed = parse(&owner, length, circular, 0, &mut state).expect("parse features");
        (parsed, state)
    }

    #[test]
    fn parses_corpus_shape_features() {
        let xml = r##"<?xml version="1.0"?><Features nextValidID="2">
            <Feature recentID="0" name="MCS" type="misc_feature" allowSegmentOverlaps="0">
              <Segment range="158-203" color="#99ccff" type="standard"/>
              <Q name="note"><V text="&lt;html&gt;&lt;body&gt;multiple cloning site&lt;/body&gt;&lt;/html&gt;"/></Q>
            </Feature>
            <Feature recentID="1" name="T7 promoter" directionality="2" type="promoter">
              <Segment range="368-386" color="#ffffff" type="standard"/>
              <Q name="note"><V text="promoter"/></Q>
            </Feature>
          </Features>"##;
        let (parsed, state) = parse_features(xml, 5369, true);
        assert_eq!(parsed.features.len(), 2);
        assert_eq!(parsed.features[0].key, "misc_feature");
        assert_eq!(parsed.features[0].name.as_deref(), Some("MCS"));
        assert_eq!(parsed.features[0].location.strand, 1);
        assert_eq!(
            parsed.features[0].location.operator,
            sequon_model::OPERATOR_SPAN
        );
        assert_eq!(parsed.features[0].location.segments[0].start, 157);
        assert_eq!(parsed.features[0].location.segments[0].end, 203);
        assert_eq!(
            parsed.features[0].qualifiers[0].value.as_deref(),
            Some("multiple cloning site")
        );
        assert_eq!(parsed.features[1].location.strand, -1);
        assert_eq!(parsed.extras.len(), 2);
        assert!(state.diagnostics.is_empty());
        assert!(state.dropped.is_empty());
    }

    #[test]
    fn keeps_a_circular_wrap_unwrapped_with_the_span_operator() {
        // A single Segment range crossing the origin of a circular record stays
        // one unwrapped span segment, so the operator is span, not join.
        let xml = r##"<Features><Feature type="misc_feature"><Segment range="90-110"/></Feature></Features>"##;
        let (parsed, _state) = parse_features(xml, 100, true);
        let location = &parsed.features[0].location;
        assert_eq!(location.operator, sequon_model::OPERATOR_SPAN);
        assert_eq!(location.segments.len(), 1);
        assert_eq!(location.segments[0].start, 89);
        assert_eq!(location.segments[0].end, 110);
        assert!(location.is_wrapped(100));
    }

    #[test]
    fn joins_multiple_segments_in_document_order() {
        let xml = r##"<Features><Feature type="CDS" directionality="2">
            <Segment range="10-20"/><Segment range="30-40"/></Feature></Features>"##;
        let (parsed, _state) = parse_features(xml, 100, true);
        let location = &parsed.features[0].location;
        assert_eq!(location.operator, sequon_model::OPERATOR_JOIN);
        assert_eq!(location.segments[0].start, 9);
        assert_eq!(location.segments[1].start, 29);
        assert_eq!(location.strand, -1);
    }

    #[test]
    fn skips_feature_without_type_and_records_it() {
        let xml = r##"<Features><Feature name="x"><Segment range="1-4"/></Feature></Features>"##;
        let (parsed, state) = parse_features(xml, 100, false);
        assert!(parsed.features.is_empty());
        assert_eq!(state.dropped.len(), 1);
        assert_eq!(state.dropped[0].reason, "malformed_metadata");
        assert_eq!(state.diagnostics[0].code, "DNA_FEATURE_MALFORMED");
    }

    #[test]
    fn skips_feature_with_out_of_range_coordinate() {
        let xml =
            r##"<Features><Feature type="CDS"><Segment range="90-120"/></Feature></Features>"##;
        let (parsed, state) = parse_features(xml, 100, false);
        assert!(parsed.features.is_empty());
        assert_eq!(state.dropped[0].reason, "coordinate_out_of_range");
        assert_eq!(state.diagnostics[0].code, "DNA_FEATURE_RANGE_INVALID");
    }

    #[test]
    fn records_int_qualifier_variant_and_raw_value() {
        let xml = r##"<Features><Feature type="CDS"><Segment range="1-3"/>
            <Q name="codon_start"><V int="1"/></Q></Feature></Features>"##;
        let (parsed, _state) = parse_features(xml, 100, false);
        assert_eq!(parsed.features[0].qualifiers[0].value.as_deref(), Some("1"));
        let extras = &parsed.extras[0];
        let qualifiers = extras
            .as_object()
            .and_then(|object| object.get("qualifiers"))
            .and_then(JsonValue::as_array)
            .expect("qualifiers extras");
        assert_eq!(qualifiers.len(), 1);
        assert_eq!(
            qualifiers[0]
                .as_object()
                .and_then(|object| object.get("value_type"))
                .and_then(JsonValue::as_str),
            Some("int")
        );
    }
}
