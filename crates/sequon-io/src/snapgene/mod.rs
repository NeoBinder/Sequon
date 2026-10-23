//! SnapGene .dna adapter (import-only).
//!
//! The accepted mapping and the verified format facts are in
//! docs/design/snapgene-dna.md. The container is one cookie packet followed by a
//! flat TLV stream; the record is produced from the DNA packet and the XML
//! metadata blocks, while blocks the canonical model cannot express are
//! preserved verbatim (or fingerprinted) in the import report.
//!
//! Every parse path treats the input as untrusted: no unwrap/expect/panic, every
//! allocation is checked against a configured limit, and every loss is reported
//! as a typed error or as a diagnostic/dropped entry.

mod codec;
mod container;
mod dna;
mod features;
mod history;
mod html;
mod notes;
mod primers;
mod properties;
mod state;
mod xml;

use std::collections::BTreeMap;

use sequon_model::{
    Digests, Extras, Feature, JsonValue, Kind, Producer, ProducerKind, Provenance, Record,
    SourceFormat, Topology, DIGEST_ALGORITHM_V1, SCHEMA_ID,
};

use crate::api::{ImportOptions, ImportOutcome, ImportResult};
use crate::error::{ImportError, RequiredField};
use crate::report::{ImportReport, RecordSummary, SourcePosition};

use state::ImportState;

/// Sequence type that selects DNA (corpus-verified).
const SEQUENCE_TYPE_DNA: u16 = 1;

/// Flags bit 0: set means circular (corpus-verified for the set case).
const FLAGS_TOPOLOGY_BIT: u8 = 0x01;

/// Flags bits observed in the corpus; a set bit outside this mask is reported.
const OBSERVED_FLAGS_MASK: u8 = 0x1f;

/// Where the record name came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NameSource {
    AccessionNumber,
    CustomMapLabel,
    Comments,
    Hint,
}

/// Imports one SnapGene .dna input.
pub fn import(bytes: &[u8], options: &ImportOptions) -> ImportResult {
    let limits = &options.limits;
    if bytes.len() as u64 > limits.max_input_bytes {
        return Err(ImportError::LimitExceeded {
            limit: "max_input_bytes",
            limit_value: limits.max_input_bytes,
            actual: bytes.len() as u64,
            position: None,
        });
    }
    let mut state = ImportState::new(limits, options.preserve_unhandled_blocks);
    let container = container::read(bytes, limits, &mut state.diagnostics)?;
    if container.cookie.sequence_type != SEQUENCE_TYPE_DNA {
        return Err(ImportError::Unsupported {
            code: "DNA_UNKNOWN_SEQ_TYPE",
            message: format!(
                "cookie sequence type {} is not the verified DNA value 1",
                container.cookie.sequence_type
            ),
            position: Some(source_position(0)),
        });
    }

    let dna_packet = select_dna_packet(&container.packets)?;
    let decoded = match dna_packet.packet_type {
        0 => dna::decode_type0(dna_packet, limits)?,
        _ => dna::decode_type1(dna_packet, limits)?,
    };
    if decoded.normalized {
        state.info(
            "DNA_SEQUENCE_NORMALIZED",
            "lowercase letters or whitespace in the DNA packet were normalized",
            Some(dna_packet.position()),
        );
    }
    let sequence_length = decoded.sequence.len() as u64;
    let topology = topology_from_flags(
        Some(decoded.flags),
        options.topology_hint,
        dna_packet.position(),
        &mut state,
    )?;
    let circular = topology == Topology::Circular;

    let mut features: Vec<Feature> = Vec::new();
    let mut feature_extras: Vec<JsonValue> = Vec::new();
    let mut primer_extras: Vec<JsonValue> = Vec::new();
    let mut primer_settings: Vec<JsonValue> = Vec::new();
    let mut primers_seen = false;
    let mut notes_data: Option<notes::NotesData> = None;
    let mut notes_extras: Vec<JsonValue> = Vec::new();
    let mut notes_position: Option<SourcePosition> = None;
    let mut properties_extras: Vec<JsonValue> = Vec::new();
    let mut history_metadata: Option<JsonValue> = None;

    for packet in &container.packets {
        match packet.packet_type {
            0 | 1 => {}
            3 => {
                state.info(
                    "DNA_PACKET_IGNORED",
                    "restriction-site metadata is preserved but not mapped into the model",
                    Some(packet.position()),
                );
                state.preserve_packet("restriction_site_list", packet);
            }
            5 => {
                state.reserve_xml_bytes(packet, packet.payload.len() as u64)?;
                primers_seen = true;
                let parsed = primers::parse(
                    packet,
                    sequence_length,
                    circular,
                    features.len(),
                    &mut state,
                )?;
                features.extend(parsed.features);
                primer_extras.extend(parsed.extras);
                primer_settings.extend(parsed.settings);
            }
            6 => {
                state.reserve_xml_bytes(packet, packet.payload.len() as u64)?;
                let parsed = notes::parse(packet, &state)?;
                if let JsonValue::Array(fields) = parsed.extras.clone() {
                    notes_extras.extend(fields);
                }
                if notes_position.is_none() {
                    notes_position = Some(packet.position());
                }
                if notes_data.is_none() {
                    notes_data = Some(parsed);
                }
            }
            8 => {
                state.reserve_xml_bytes(packet, packet.payload.len() as u64)?;
                let parsed = properties::parse(packet, &state)?;
                if let JsonValue::Array(fields) = parsed.extras {
                    properties_extras.extend(fields);
                }
                if let Some(properties_topology) = parsed.topology {
                    if properties_topology != topology {
                        state.warning(
                            "DNA_TOPOLOGY_CONFLICT",
                            "notes/properties topology disagrees with the DNA flags; the flags win",
                            Some(packet.position()),
                        );
                    }
                }
            }
            10 => {
                state.reserve_xml_bytes(packet, packet.payload.len() as u64)?;
                let parsed = features::parse(
                    packet,
                    sequence_length,
                    circular,
                    features.len(),
                    &mut state,
                )?;
                features.extend(parsed.features);
                feature_extras.extend(parsed.extras);
            }
            7 | 29 => {
                history_metadata = Some(history::process(
                    packet,
                    container.cookie.export_version,
                    &mut state,
                )?);
            }
            13 => {
                state.info(
                    "DNA_PACKET_IGNORED",
                    "enzyme settings are preserved but not mapped into the model",
                    Some(packet.position()),
                );
                state.preserve_packet("enzyme_settings", packet);
            }
            16 => {
                state.info(
                    "DNA_PACKET_IGNORED",
                    "trace container is preserved or fingerprinted, not interpreted",
                    Some(packet.position()),
                );
                state.preserve_packet("trace_container", packet);
            }
            17 => {
                state.info(
                    "DNA_PACKET_IGNORED",
                    "alignable sequences are preserved or fingerprinted, not interpreted",
                    Some(packet.position()),
                );
                state.preserve_packet("alignable_sequence", packet);
            }
            other => {
                state.info(
                    "DNA_UNKNOWN_PACKET",
                    format!("packet type {other} is unknown; its payload is preserved"),
                    Some(packet.position()),
                );
                state.preserve_packet("unknown_packet", packet);
            }
        }
    }

    let (name, name_source) = derive_name(
        notes_data.as_ref(),
        options.name_hint.as_deref(),
        notes_position,
    )?;
    let description = derive_description(notes_data.as_ref(), name_source);

    let sequence = &decoded.sequence;
    let sequence_digest =
        sequon_algo::sequence_digest(sequence).map_err(|error| ImportError::Malformed {
            code: "DNA_SEQUENCE_INVALID_CHAR",
            message: format!("the decoded sequence is not a valid normalized sequence: {error}"),
            position: None,
        })?;
    let canonical_digest = sequon_algo::canonical_digest(sequence, topology).map_err(|error| {
        ImportError::Malformed {
            code: "DNA_SEQUENCE_INVALID_CHAR",
            message: format!("the canonical digest cannot be computed: {error}"),
            position: None,
        }
    })?;
    let source_digest = sequon_algo::bytes_digest(bytes);

    let extras = assemble_extras(ExtrasParts {
        flags: decoded.flags,
        feature_extras,
        primers_seen,
        primer_extras,
        primer_settings,
        notes_extras,
        properties_extras,
        history_metadata,
    });
    let record = Record {
        schema: SCHEMA_ID.to_string(),
        kind: Kind::Dna,
        molecule_type: None,
        topology,
        name: name.clone(),
        description,
        length: sequence_length,
        sequence: decoded.sequence,
        digests: Digests {
            algorithm: DIGEST_ALGORITHM_V1.to_string(),
            sequence: sequence_digest.clone(),
            canonical: canonical_digest.clone(),
        },
        features,
        provenance: Provenance {
            producer: Producer {
                kind: ProducerKind::Parser,
                name: state::PRODUCER_NAME.to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            source_format: SourceFormat::SnapgeneDna,
            source_digest,
            source_bytes: bytes.len() as u64,
            export_version: Some(u32::from(container.cookie.export_version)),
        },
        extras,
    };
    record.validate().map_err(|error| ImportError::Malformed {
        code: "RECORD_FIELD_INVALID",
        message: format!("the parsed record failed validation: {error}"),
        position: None,
    })?;

    let mut report = ImportReport::new(
        SourceFormat::SnapgeneDna,
        sequon_algo::bytes_digest(bytes),
        bytes.len() as u64,
        Producer {
            kind: ProducerKind::Parser,
            name: state::PRODUCER_NAME.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
        Some(u32::from(container.cookie.export_version)),
    );
    report.records.push(RecordSummary {
        index: state::RECORD_INDEX,
        name,
        kind: Kind::Dna,
        length: sequence_length,
        sequence_digest,
        canonical_digest,
    });
    report.diagnostics = state.diagnostics;
    report.dropped = state.dropped;
    report.preserved = state.preserved;
    report.sort_diagnostics();
    Ok(ImportOutcome {
        records: vec![record],
        report,
    })
}

fn select_dna_packet<'a>(
    packets: &'a [container::Packet<'a>],
) -> Result<&'a container::Packet<'a>, ImportError> {
    let mut selected: Option<&container::Packet<'a>> = None;
    for packet in packets {
        if packet.packet_type != 0 && packet.packet_type != 1 {
            continue;
        }
        if let Some(previous) = selected {
            return Err(ImportError::Malformed {
                code: "DNA_DUPLICATE_DNA_PACKET",
                message: format!(
                    "DNA packet type {} follows DNA packet type {}; no implicit choice is made",
                    packet.packet_type, previous.packet_type
                ),
                position: Some(packet.position()),
            });
        }
        selected = Some(packet);
    }
    selected.ok_or(ImportError::Malformed {
        code: "DNA_NO_DNA_PACKET",
        message: "the container has neither an uncompressed nor a compressed DNA packet"
            .to_string(),
        position: None,
    })
}

fn topology_from_flags(
    flags: Option<u8>,
    hint: Option<Topology>,
    position: SourcePosition,
    state: &mut ImportState<'_>,
) -> Result<Topology, ImportError> {
    match flags {
        Some(flags) => {
            let unknown = flags & !OBSERVED_FLAGS_MASK;
            if unknown != 0 {
                state.info(
                    "DNA_TOPOLOGY_FLAG_UNKNOWN",
                    format!(
                        "DNA flags 0x{flags:02x} set bit(s) 0x{unknown:02x} outside the observed mask"
                    ),
                    Some(position),
                );
            }
            Ok(if flags & FLAGS_TOPOLOGY_BIT != 0 {
                Topology::Circular
            } else {
                Topology::Linear
            })
        }
        None => match hint {
            Some(hint) => Ok(hint),
            None => Err(ImportError::MissingRequired {
                field: RequiredField::Topology,
                position: Some(position),
            }),
        },
    }
}

fn derive_name(
    notes: Option<&notes::NotesData>,
    hint: Option<&str>,
    notes_position: Option<SourcePosition>,
) -> Result<(String, NameSource), ImportError> {
    if let Some(notes) = notes {
        if let Some(value) = non_empty(notes.accession.as_deref()) {
            return Ok((value, NameSource::AccessionNumber));
        }
        if notes.use_custom_map_label {
            if let Some(value) = non_empty(notes.custom_map_label.as_deref()) {
                return Ok((value, NameSource::CustomMapLabel));
            }
        }
        if let Some(value) = non_empty(notes.comments.as_deref()) {
            return Ok((value, NameSource::Comments));
        }
    }
    if let Some(value) = non_empty(hint) {
        return Ok((value, NameSource::Hint));
    }
    Err(ImportError::MissingRequired {
        field: RequiredField::Name,
        position: notes_position,
    })
}

fn derive_description(notes: Option<&notes::NotesData>, source: NameSource) -> Option<String> {
    let notes = notes?;
    if let Some(value) = non_empty(notes.description.as_deref()) {
        return Some(value);
    }
    if source != NameSource::Comments {
        if let Some(value) = non_empty(notes.comments.as_deref()) {
            return Some(value);
        }
    }
    None
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

struct ExtrasParts {
    flags: u8,
    feature_extras: Vec<JsonValue>,
    primers_seen: bool,
    primer_extras: Vec<JsonValue>,
    primer_settings: Vec<JsonValue>,
    notes_extras: Vec<JsonValue>,
    properties_extras: Vec<JsonValue>,
    history_metadata: Option<JsonValue>,
}

fn assemble_extras(parts: ExtrasParts) -> Extras {
    let ExtrasParts {
        flags,
        feature_extras,
        primers_seen,
        primer_extras,
        primer_settings,
        notes_extras,
        properties_extras,
        history_metadata,
    } = parts;
    let mut snapgene: BTreeMap<String, JsonValue> = BTreeMap::new();
    snapgene.insert("dna_flags".to_string(), JsonValue::from(u64::from(flags)));
    if !feature_extras.is_empty() {
        snapgene.insert("features".to_string(), JsonValue::Array(feature_extras));
    }
    if primers_seen {
        snapgene.insert("primers".to_string(), JsonValue::Array(primer_extras));
    }
    if !primer_settings.is_empty() {
        snapgene.insert(
            "primer_settings".to_string(),
            JsonValue::Array(primer_settings),
        );
    }
    if !notes_extras.is_empty() {
        snapgene.insert("notes".to_string(), JsonValue::Array(notes_extras));
    }
    if !properties_extras.is_empty() {
        snapgene.insert(
            "properties".to_string(),
            JsonValue::Array(properties_extras),
        );
    }
    if let Some(history) = history_metadata {
        snapgene.insert("history".to_string(), history);
    }
    let mut extras = Extras::new();
    extras.insert("snapgene", JsonValue::Object(snapgene));
    extras
}

fn source_position(offset: u64) -> SourcePosition {
    SourcePosition::Snapgene {
        offset,
        packet_type: None,
        packet_index: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::Limits;

    fn empty_state() -> ImportState<'static> {
        let limits: &'static Limits = Box::leak(Box::new(Limits::default()));
        ImportState::new(limits, true)
    }

    #[test]
    fn missing_topology_falls_back_to_hint_then_errors() {
        let mut state = empty_state();
        let hint =
            topology_from_flags(None, Some(Topology::Linear), source_position(0), &mut state)
                .expect("hint");
        assert_eq!(hint, Topology::Linear);
        let error =
            topology_from_flags(None, None, source_position(0), &mut state).expect_err("missing");
        assert_eq!(error.code(), "RECORD_REQUIRED_FIELD_MISSING");
    }

    #[test]
    fn unknown_flag_bits_are_reported_but_do_not_change_topology() {
        let mut state = empty_state();
        let topology = topology_from_flags(Some(0x21), None, source_position(0), &mut state)
            .expect("topology");
        assert_eq!(topology, Topology::Circular);
        assert_eq!(state.diagnostics.len(), 1);
        assert_eq!(state.diagnostics[0].code, "DNA_TOPOLOGY_FLAG_UNKNOWN");
    }

    #[test]
    fn name_precedence_matches_the_contract() {
        let mut data = notes::NotesData {
            extras: JsonValue::Null,
            accession: None,
            custom_map_label: Some("pET-28a(+)".to_string()),
            use_custom_map_label: true,
            description: None,
            comments: None,
        };
        let (name, source) = derive_name(Some(&data), Some("hint"), None).expect("name");
        assert_eq!(name, "pET-28a(+)");
        assert_eq!(source, NameSource::CustomMapLabel);

        data.accession = Some("ACC.1".to_string());
        let (name, source) = derive_name(Some(&data), None, None).expect("name");
        assert_eq!(name, "ACC.1");
        assert_eq!(source, NameSource::AccessionNumber);

        data.accession = None;
        data.use_custom_map_label = false;
        data.comments = Some("plain comment".to_string());
        let (name, source) = derive_name(Some(&data), None, None).expect("name");
        assert_eq!(name, "plain comment");
        assert_eq!(source, NameSource::Comments);
    }

    #[test]
    fn description_skips_comments_used_as_name() {
        let data = notes::NotesData {
            extras: JsonValue::Null,
            accession: None,
            custom_map_label: None,
            use_custom_map_label: false,
            description: None,
            comments: Some("comment text".to_string()),
        };
        assert_eq!(derive_description(Some(&data), NameSource::Comments), None);
        assert_eq!(
            derive_description(Some(&data), NameSource::Hint).as_deref(),
            Some("comment text")
        );
    }

    #[test]
    fn extras_include_flags_always() {
        let extras = assemble_extras(ExtrasParts {
            flags: 0x1f,
            feature_extras: Vec::new(),
            primers_seen: false,
            primer_extras: Vec::new(),
            primer_settings: Vec::new(),
            notes_extras: Vec::new(),
            properties_extras: Vec::new(),
            history_metadata: None,
        });
        let snapgene = extras
            .get("snapgene")
            .and_then(JsonValue::as_object)
            .expect("snapgene");
        assert_eq!(
            snapgene.get("dna_flags").and_then(JsonValue::as_u64),
            Some(31)
        );
    }
}
