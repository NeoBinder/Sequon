//! Canonical data model for Sequon.
//!
//! This crate owns the in-memory representation that every format adapter
//! produces and every consumer reads. It knows nothing about file formats and
//! performs no IO.
//!
//! The authoritative semantics live in docs/design/canonical-model.md; the
//! byte-level serialization rules live in docs/design/canonical-json-profile.md;
//! the machine-readable copy of the envelope is
//! schemas/sequon-record-1.schema.json. When the artifacts disagree, the
//! accepted document wins and the others change in the same commit.
//!
//! The crate depends on serde only. It stores digest values and validates their
//! form, but it never hashes: computing or verifying a digest is the job of
//! sequon-algo, which owns the hashing dependency. That split is why
//! Record::validate() checks digest syntax while sequon_algo::verify_digests()
//! checks digest values.
//!
//! Serialization rules that the type definitions encode:
//!
//! - struct field order is the canonical JSON field order;
//! - nullable optional fields are plain Option fields without
//!   skip_serializing_if, so they are always emitted and serialized as null
//!   when absent;
//! - every canonical struct denies unknown fields, while extras is free-form;
//! - extras object keys are sorted because the payload is a BTreeMap;
//! - integers are stored as integers and floating-point numbers are not
//!   representable anywhere, including inside extras.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

mod digest;
mod extras;
mod feature;
mod location;
mod provenance;
mod qualifier;
mod record;
mod schema;
mod validate;

pub use digest::{DigestFormatError, DigestValue, Digests, DIGEST_ALGORITHM_V1};
pub use extras::{Extras, JsonValue};
pub use feature::Feature;
pub use location::{Location, Segment, OPERATOR_JOIN, OPERATOR_ORDER, OPERATOR_SPAN};
pub use provenance::{Producer, ProducerKind, Provenance, SourceFormat};
pub use qualifier::Qualifier;
pub use record::{Kind, MoleculeType, Record, Topology, SCHEMA_ID};
pub use schema::{RECORD_SCHEMA, RECORD_SCHEMA_URN};
pub use validate::ValidationError;
