//! Shared fixture helpers for the sequon-algo integration test binaries.
//!
//! Each integration test binary compiles its own copy of this module, so a
//! helper used by only some binaries is expected and not dead library code.
#![allow(dead_code)]

use sequon_algo::{canonical_digest, sequence_digest};
use sequon_model::{
    DigestValue, Digests, Extras, Feature, Kind, Location, Producer, ProducerKind, Provenance,
    Qualifier, Record, SourceFormat, Topology, DIGEST_ALGORITHM_V1,
};

pub fn qualifiers(values: &[(&str, &str)]) -> Vec<Qualifier> {
    values
        .iter()
        .map(|(key, value)| Qualifier::new(*key, *value))
        .collect()
}

pub fn cds(location: Location, qualifiers: Vec<Qualifier>) -> Feature {
    Feature {
        key: "CDS".to_owned(),
        so_term: Some("SO:0000316".to_owned()),
        name: None,
        location,
        qualifiers,
    }
}

pub fn make_record(sequence: &str, topology: Topology, features: Vec<Feature>) -> Record {
    Record {
        schema: "sequon/record/1".to_owned(),
        kind: Kind::Dna,
        molecule_type: None,
        topology,
        name: "analysis-test".to_owned(),
        description: None,
        length: sequence.len() as u64,
        sequence: sequence.to_owned(),
        digests: Digests {
            algorithm: DIGEST_ALGORITHM_V1.to_owned(),
            sequence: sequence_digest(sequence).expect("valid test sequence"),
            canonical: canonical_digest(sequence, topology).expect("valid test sequence"),
        },
        features,
        provenance: Provenance {
            producer: Producer {
                kind: ProducerKind::Parser,
                name: "sequon-test".to_owned(),
                version: "1".to_owned(),
            },
            source_format: SourceFormat::Genbank,
            source_digest: DigestValue::parse(
                "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            )
            .expect("valid digest"),
            source_bytes: 0,
            export_version: None,
        },
        extras: Extras::new(),
    }
}
