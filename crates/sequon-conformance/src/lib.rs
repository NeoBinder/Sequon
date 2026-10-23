//! Development-only conformance harness for Sequon.
//!
//! This crate owns the checks that keep the contract honest: JSON Schema
//! validation of committed examples and goldens, determinism and round-trip
//! assertions, cross-format comparison of a SnapGene `.dna` file with its
//! "Save As" GenBank export, and the fixture manifest rules in
//! `testdata/README.md`.
//!
//! It is a development tool. It is `publish = false`, no runtime consumer
//! depends on it, and it is never a dependency of the library crates.
//!
//! Modules:
//!
//! - [`dna_builder`]: a deterministic builder for synthetic SnapGene
//!   containers. It is a fixture generator, not a production writer: the
//!   adapter is import-only (D14), so nothing here is reachable from
//!   `sequon-io`.
//! - [`snapgene_fixtures`]: the committed-fixture catalogue that the builder
//!   produces and `tests/snapgene_fixtures.rs` verifies byte for byte.
//!
//! The schema side lives in `tests/schema_validation.rs`, which compiles
//! `schemas/sequon-record-1.schema.json` and
//! `schemas/import-report-1.schema.json` and validates every example in
//! `schemas/examples/`, plus negative cases that must be rejected.
#![deny(missing_docs)]

pub mod dna_builder;
pub mod snapgene_fixtures;
