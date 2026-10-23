//! Determinism and shape gate for the committed SnapGene fixtures.
//!
//! This test is the fixture gate, not an adapter test:
//!
//! - every committed file under `testdata/snapgene/` must equal the bytes
//!   the builder produces today (byte determinism);
//! - the sha256 recorded in `testdata/README.md` must equal the sha256 of
//!   those bytes (manifest determinism);
//! - a small independent container reader in this file walks the cookie and
//!   TLV stream and asserts the structural facts each fixture exists to pin.
//!
//! The independent reader is deliberately naive and exists only here. It is
//! not the adapter, it is not exported, and it does not interpret the
//! canonical model.

use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use sequon_conformance::snapgene_fixtures::{all, TWIN_SEQUENCE};

const COOKIE_PAYLOAD_LEN: usize = 14;

/// A container parsed just enough to assert fixture shape.
struct Parsed<'a> {
    sequence_type: u16,
    export_version: u16,
    import_version: u16,
    packets: Vec<(u8, &'a [u8])>,
}

impl Parsed<'_> {
    fn packet(&self, packet_type: u8) -> Option<&[u8]> {
        self.packets
            .iter()
            .find(|(kind, _)| *kind == packet_type)
            .map(|(_, payload)| *payload)
    }

    fn packet_types(&self) -> Vec<u8> {
        self.packets.iter().map(|(kind, _)| *kind).collect()
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate lives at <workspace>/crates/sequon-conformance")
        .to_path_buf()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// Minimal independent cookie + TLV walker.
///
/// Returns an error string for any declared length that reaches past the end
/// of the file; that is the shape fixture 070 exists to pin.
fn parse_container(bytes: &[u8]) -> Result<Parsed<'_>, String> {
    if bytes.len() < 5 {
        return Err(format!(
            "file is {} bytes, shorter than a packet header",
            bytes.len()
        ));
    }
    if bytes[0] != 0x09 {
        return Err(format!("first packet type is 0x{:02x}, not 0x09", bytes[0]));
    }
    let declared = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize;
    if declared < COOKIE_PAYLOAD_LEN || 5 + declared > bytes.len() {
        return Err(format!("cookie length {declared} does not fit the file"));
    }
    let payload = &bytes[5..5 + declared];
    if !payload.starts_with(b"SnapGene") {
        return Err("cookie payload does not start with SnapGene".to_owned());
    }
    let sequence_type = u16::from_be_bytes([payload[8], payload[9]]);
    let export_version = u16::from_be_bytes([payload[10], payload[11]]);
    let import_version = u16::from_be_bytes([payload[12], payload[13]]);

    let mut packets = Vec::new();
    let mut offset = 5 + declared;
    while offset < bytes.len() {
        if bytes.len() - offset < 5 {
            return Err(format!(
                "less than one packet header remains at offset {offset}"
            ));
        }
        let packet_type = bytes[offset];
        let length = u32::from_be_bytes([
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
            bytes[offset + 4],
        ]) as usize;
        let payload_start = offset + 5;
        if payload_start + length > bytes.len() {
            return Err(format!(
                "truncated packet type {packet_type} at offset {offset}: declared {length}, available {}",
                bytes.len() - payload_start
            ));
        }
        packets.push((packet_type, &bytes[payload_start..payload_start + length]));
        offset = payload_start + length;
    }
    Ok(Parsed {
        sequence_type,
        export_version,
        import_version,
        packets,
    })
}

fn type0_bases(parsed: &Parsed<'_>) -> String {
    let payload = parsed.packet(0).expect("type 0 packet present");
    let bases = std::str::from_utf8(&payload[1..]).expect("type 0 bases are UTF-8");
    bases.to_ascii_uppercase()
}

/// Independent decoder for the builder convention, used only to prove that
/// the type-1 twin encodes the same molecule as the type-0 twin.
fn decode_twobit(payload: &[u8]) -> String {
    let count = u32::from_be_bytes([payload[1], payload[2], payload[3], payload[4]]) as usize;
    let packed = &payload[5..];
    let mut bases = String::with_capacity(count);
    for index in 0..count {
        let byte = packed[index / 4];
        let shift = 6 - 2 * (index % 4);
        bases.push(match (byte >> shift) & 0b11 {
            0 => 'G',
            1 => 'A',
            2 => 'T',
            _ => 'C',
        });
    }
    bases
}

#[test]
fn catalogue_is_unique_sorted_and_complete() {
    let fixtures = all();
    assert_eq!(fixtures.len(), 31, "expected the committed fixture set");
    let mut names: Vec<&str> = fixtures.iter().map(|fixture| fixture.name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), fixtures.len(), "fixture names are unique");
    let ordered: Vec<&str> = fixtures.iter().map(|fixture| fixture.name).collect();
    let mut sorted = ordered.clone();
    sorted.sort_unstable();
    assert_eq!(ordered, sorted, "all() must be sorted by file name");
}

#[test]
fn committed_fixtures_match_the_builder_and_manifest() {
    let readme = fs::read_to_string(workspace_root().join("testdata/README.md"))
        .expect("testdata/README.md is readable");
    let directory = workspace_root().join("testdata/snapgene");

    for fixture in all() {
        // Building twice must produce identical bytes.
        let rebuilt = all()
            .into_iter()
            .find(|candidate| candidate.name == fixture.name)
            .expect("fixture is in the catalogue");
        assert_eq!(
            rebuilt.bytes, fixture.bytes,
            "{} is not deterministic",
            fixture.name
        );

        let committed = fs::read(directory.join(fixture.name))
            .unwrap_or_else(|error| panic!("read {}: {error}", fixture.name));
        assert_eq!(
            committed, fixture.bytes,
            "{} does not match the builder; regenerate deliberately with gen-snapgene-fixtures",
            fixture.name
        );

        let digest = sha256_hex(&fixture.bytes);
        let row = readme
            .lines()
            .find(|line| line.starts_with(&format!("| `{}` |", fixture.name)))
            .unwrap_or_else(|| panic!("{} has no row in testdata/README.md", fixture.name));
        assert!(
            row.contains(&digest),
            "{} row does not record sha256 {digest}: {row}",
            fixture.name
        );
    }
}

#[test]
fn container_shape_is_independently_parsed() {
    for fixture in all() {
        let name = fixture.name;
        let parsed = match parse_container(&fixture.bytes) {
            Ok(parsed) => parsed,
            Err(error) => {
                assert_eq!(name, "070-truncated.dna", "{name} failed to parse: {error}");
                assert!(
                    error.contains("truncated packet"),
                    "unexpected error: {error}"
                );
                continue;
            }
        };
        assert_eq!(parsed.import_version, 19, "{name}: import version");

        match name {
            "001-linear-uncompressed.dna" => {
                assert_eq!(parsed.sequence_type, 1);
                assert_eq!(parsed.export_version, 15);
                assert_eq!(parsed.packet_types(), vec![0, 6, 10]);
                assert_eq!(parsed.packet(0).expect("dna")[0], 0x00);
                assert_eq!(type0_bases(&parsed), TWIN_SEQUENCE);
            }
            "002-circular-compressed.dna" => {
                assert_eq!(parsed.packet_types(), vec![1, 6]);
                let payload = parsed.packet(1).expect("2-bit dna");
                assert_eq!(payload[0], 0x01);
                assert_eq!(decode_twobit(payload), TWIN_SEQUENCE);
            }
            "003-type0-observed-flags.dna" => {
                assert_eq!(parsed.packet(0).expect("dna")[0], 0x1f);
            }
            "004-linear-zero-flags.dna" => {
                assert_eq!(parsed.packet(0).expect("dna")[0], 0x00);
            }
            "005-unknown-flag-bits.dna" => {
                assert_eq!(parsed.packet(0).expect("dna")[0], 0x21);
            }
            "010-history-v14.dna" => {
                assert!(parsed.packet_types().contains(&7));
                let history = parsed.packet(7).expect("history");
                assert!(history.starts_with(b"<?xml"), "v14 history is plain XML");
            }
            "011-history-v17.dna" => {
                assert!(parsed.packet_types().contains(&7));
                let history = parsed.packet(7).expect("history");
                assert!(history.len() > COOKIE_PAYLOAD_LEN);
                assert_ne!(history[0], b'<', "v17 history is LZMA-compressed");
            }
            "012-version15-no-history.dna" => {
                assert_eq!(parsed.export_version, 15);
                assert!(!parsed.packet_types().contains(&7));
            }
            "013-unsorted-packets.dna" => {
                assert_eq!(parsed.packet_types(), vec![6, 0, 8]);
            }
            "020-features-multisegment.dna" => {
                let xml = std::str::from_utf8(parsed.packet(10).expect("features"))
                    .expect("features XML is UTF-8");
                assert!(xml.contains("range=\"20-30\""));
                assert!(xml.contains("range=\"5-10\""));
                assert!(xml.contains("directionality=\"2\""));
            }
            "021-features-attributes.dna" => {
                let xml = std::str::from_utf8(parsed.packet(10).expect("features"))
                    .expect("features XML is UTF-8");
                assert!(xml.contains("<V text="));
                assert!(xml.contains("<V int="));
                assert!(xml.contains("&lt;html&gt;"));
                assert!(xml.contains("color="));
            }
            "030-primers.dna" => {
                let xml = std::str::from_utf8(parsed.packet(5).expect("primers"))
                    .expect("primers XML is UTF-8");
                assert!(xml.contains("<Primer "));
                assert!(xml.contains("name=\"M13F\""));
                assert!(xml.contains("range=\"1-17\""));
            }
            "031-primers-empty-settings.dna" => {
                let xml = std::str::from_utf8(parsed.packet(5).expect("primers"))
                    .expect("primers XML is UTF-8");
                assert!(xml.contains("HybridizationParams"));
                assert!(!xml.contains("<Primer "));
            }
            "040-notes-properties.dna" => {
                let notes =
                    std::str::from_utf8(parsed.packet(6).expect("notes")).expect("notes XML");
                assert!(notes.contains("<Description>"));
                let properties =
                    std::str::from_utf8(parsed.packet(8).expect("properties")).expect("props XML");
                assert!(properties.contains("<UpstreamStickiness>0</UpstreamStickiness>"));
            }
            "041-name-custom-map-label.dna" => {
                let notes =
                    std::str::from_utf8(parsed.packet(6).expect("notes")).expect("notes XML");
                assert!(notes.contains("CustomMapLabel"));
                assert!(notes.contains("UseCustomMapLabel>1"));
            }
            "042-name-precedence.dna" => {
                let notes =
                    std::str::from_utf8(parsed.packet(6).expect("notes")).expect("notes XML");
                assert!(notes.contains("AccessionNumber"));
                assert!(notes.contains("UseCustomMapLabel>0"));
            }
            "050-traces.dna" => {
                assert!(parsed.packet_types().contains(&16));
            }
            "060-unknown-packet.dna" => {
                assert!(parsed.packet_types().contains(&42));
            }
            "061-restriction-sites.dna" => {
                let payload = parsed.packet(3).expect("restriction sites");
                assert_eq!(payload[0], 1);
                let list_len = u32::from_be_bytes([payload[1], payload[2], payload[3], payload[4]]);
                assert_eq!(list_len as usize, 27);
            }
            "062-enzyme-settings.dna" => {
                let payload = parsed.packet(13).expect("enzyme settings");
                assert_eq!(&payload[18..35], b"Unique 6+ Cutters");
            }
            "080-lzma-bomb.dna" => {
                let history = parsed.packet(7).expect("history");
                assert_eq!(history.len(), 21);
                let declared = u64::from_le_bytes([
                    history[5],
                    history[6],
                    history[7],
                    history[8],
                    history[9],
                    history[10],
                    history[11],
                    history[12],
                ]);
                assert_eq!(declared, 64 * 1024 * 1024 + 1);
            }
            "090-deep-xml.dna" => {
                let xml = std::str::from_utf8(parsed.packet(10).expect("features"))
                    .expect("features XML is UTF-8");
                assert_eq!(xml.matches("<N>").count(), 80);
            }
            "100-unicode-notes.dna" => {
                let bases = parsed.packet(0).expect("dna");
                let text = std::str::from_utf8(&bases[1..]).expect("bases are UTF-8");
                assert!(text.chars().all(|base| base.is_ascii_lowercase()));
                let notes = parsed.packet(6).expect("notes");
                assert!(std::str::from_utf8(notes)
                    .expect("notes UTF-8")
                    .contains("naïve"));
            }
            "110-both-dna-packets.dna" => {
                assert_eq!(parsed.packet_types(), vec![0, 1, 6]);
            }
            "120-no-dna-packet.dna" => {
                assert!(!parsed.packet_types().contains(&0));
                assert!(!parsed.packet_types().contains(&1));
            }
            "130-missing-name.dna" => {
                let notes =
                    std::str::from_utf8(parsed.packet(6).expect("notes")).expect("notes XML");
                assert!(!notes.contains("<CustomMapLabel"));
                assert!(!notes.contains("AccessionNumber"));
            }
            "063-alignable-sequences.dna" => {
                let payload = parsed.packet(17).expect("alignable sequences");
                let xml = std::str::from_utf8(payload).expect("alignable XML is UTF-8");
                assert!(xml.contains("<AlignableSequence "));
            }
            "064-history-modifier.dna" => {
                let payload = parsed.packet(29).expect("history modifier");
                assert!(payload.len() > 13);
                assert_ne!(payload[0], b'<', "export 17 modifier payload is LZMA");
            }
            "131-rna-seq-type.dna" => {
                assert_eq!(parsed.sequence_type, 2);
            }
            "132-unknown-seq-type.dna" => {
                assert_eq!(parsed.sequence_type, 0);
            }
            other => panic!("{other}: no independent shape check"),
        }
    }
}
