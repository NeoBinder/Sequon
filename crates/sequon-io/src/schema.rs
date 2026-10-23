//! The JSON Schema document owned by this crate, embedded as published bytes.
//!
//! The authoritative file is `schemas/import-report-1.schema.json` at the
//! repository root, next to the record schema owned by `sequon-model`. The crate
//! directory mirrors it as a relative symlink,
//! `crates/sequon-io/schemas/import-report-1.schema.json`; [`REPORT_SCHEMA`]
//! embeds those bytes with `include_str!`, so the repository keeps exactly one
//! authoritative document while a published crate carries a real copy. `cargo
//! package` dereferences the symlink (a direct `include_str!` of
//! `../../../schemas/...` would not survive packaging), which is why the
//! indirection exists.
//!
//! The bytes are frozen per crate release: any schema change requires a new
//! sequon-io release. Consumers should hash the embedded bytes exactly as they
//! are (for example with `sequon_algo::bytes_digest`) and pin that digest;
//! re-serializing or re-formatting the document would change it.

/// Exact bytes of the version 1 import-report JSON Schema.
///
/// This is the committed document `schemas/import-report-1.schema.json` as UTF-8
/// text, embedded unmodified: no re-serialization, no normalization and no
/// whitespace rewrite. A platform reads it at runtime to compute the SHA-256 it
/// pins beside an import report (the record pin is
/// [`sequon_model::RECORD_SCHEMA`], owned by `sequon-model`).
///
/// The bytes are frozen per sequon-io release; a schema change requires a crate
/// release, and code that hardcodes a digest of this document must be updated
/// with that release.
pub const REPORT_SCHEMA: &str = include_str!("../schemas/import-report-1.schema.json");

/// Stable `$id` of [`REPORT_SCHEMA`]: `urn:sequon:schema:import-report:1`.
///
/// This is the value of the `$id` member inside the embedded document, so a
/// consumer can verify the pin identity without parsing JSON. It is distinct
/// from [`crate::REPORT_SCHEMA_ID`], which is the `schema` field carried inside
/// an import report payload (`sequon/import-report/1`).
pub const REPORT_SCHEMA_URN: &str = "urn:sequon:schema:import-report:1";
