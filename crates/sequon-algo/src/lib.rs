//! Sequence computation for Sequon.
//!
//! Everything here operates on canonical records and has no knowledge of file
//! formats: translation, restriction analysis, ligation and assembly planning,
//! and alignment entry points. Functions are deterministic and report failures
//! through typed errors rather than panics.
//!
//! No algorithm is implemented yet; the canonical model comes first. See
//! `docs/design/canonical-model.md`.
#![deny(missing_docs)]
