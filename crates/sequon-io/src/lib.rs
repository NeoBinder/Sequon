//! Format adapters for Sequon.
//!
//! Each supported file format gets one adapter that parses into the canonical
//! model and, where the model allows it losslessly, serializes back out.
//! Adapters accept untrusted input and must report problems instead of
//! panicking or dropping data silently.
//!
//! No adapter is implemented yet; the canonical model comes first. See
//! `docs/design/canonical-model.md`.
#![deny(missing_docs)]
