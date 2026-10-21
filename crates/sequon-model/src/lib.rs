//! Canonical data model for Sequon.
//!
//! This crate owns the in-memory representation that every format adapter
//! produces and every consumer reads. It knows nothing about file formats and
//! performs no IO.
//!
//! The model is not decided yet. The authoritative discussion lives in
//! `docs/design/canonical-model.md`; do not add public types here before that
//! document is accepted.
#![deny(missing_docs)]
