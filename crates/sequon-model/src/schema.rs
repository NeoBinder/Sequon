//! The JSON Schema document owned by this crate, embedded as published bytes.
//!
//! The authoritative file is `schemas/sequon-record-1.schema.json` at the
//! repository root. The crate directory mirrors it as a relative symlink,
//! `crates/sequon-model/schemas/sequon-record-1.schema.json`; [`RECORD_SCHEMA`]
//! embeds those bytes with `include_str!`, so the repository keeps exactly one
//! authoritative document while a published crate carries a real copy. `cargo
//! package` dereferences the symlink (a direct `include_str!` of
//! `../../../schemas/...` would not survive packaging), which is why the
//! indirection exists.
//!
//! The bytes are frozen per crate release: any schema change requires a new
//! sequon-model release. Consumers should hash the embedded bytes exactly as
//! they are (for example with `sequon_algo::bytes_digest`) and pin that
//! digest; re-serializing or re-formatting the document would change it.

/// Exact bytes of the version 1 canonical record JSON Schema.
///
/// This is the committed document `schemas/sequon-record-1.schema.json` as UTF-8
/// text, embedded unmodified: no re-serialization, no normalization and no
/// whitespace rewrite. A platform reads it at runtime to compute the SHA-256 it
/// pins beside a canonical payload; the matching report-schema pin is
/// `sequon_io::REPORT_SCHEMA`, owned by the `sequon-io` crate.
///
/// The bytes are frozen per sequon-model release; a schema change requires a
/// crate release, and code that hardcodes a digest of this document must be
/// updated with that release.
pub const RECORD_SCHEMA: &str = include_str!("../schemas/sequon-record-1.schema.json");

/// Stable `$id` of [`RECORD_SCHEMA`]: `urn:sequon:schema:record:1`.
///
/// This is the value of the `$id` member inside the embedded document, so a
/// consumer can verify the pin identity without parsing JSON. It is distinct
/// from [`crate::SCHEMA_ID`], which is the `schema` field carried inside a
/// canonical record payload (`sequon/record/1`).
pub const RECORD_SCHEMA_URN: &str = "urn:sequon:schema:record:1";
