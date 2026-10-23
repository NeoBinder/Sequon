# Sequon

Sequon is a Rust framework for DNA and protein sequence processing: format IO,
a canonical data layer, and sequence computation. Consumers embed it as a
library.

## Scope

- Library workspace only. No HTTP service, no long-running daemon, no server
  endpoints, and no server framework in this repository. A consumer that needs a
  process or network boundary (for example the NeoBinder API) wraps Sequon on
  its side.
- The intermediate structure is JSON. Serialized records use the `.json`
  extension; do not invent a binary container, a custom extension, or a
  compression framing of our own.
- All repository text is English: AGENTS.md, README.md, docs, code comments,
  commit messages, issue and pull request descriptions. Chinese belongs in local
  uncommitted notes.

## Current milestone

The canonical model, schemas and diagnostics are accepted
(`docs/design/canonical-model.md`, M0). The schemas in `schemas/` are their
machine-readable copy; change the accepted document and the schema in the same
commit.

Milestones M1-M4 are implemented: the GenBank reader and writer are in
`sequon-io` (default feature), and the SnapGene `.dna` reader is behind the
`snapgene` feature (import-only). M5 conformance, CI and fuzzing are landing.

- do not add a second representation "for convenience",
- parse warnings, dropped data and preserved blocks go to the import report, never into a record,
- a record is valid for use only after `Record::validate()` and `sequon_algo::verify_digests()`,
- keep examples in the design documents, `schemas/examples/` or a crate's `examples/`.

The document is the source of truth. When the model changes, change the document
in the same commit.

## Layout

- `crates/sequon-model` — canonical types and invariants. No IO, no algorithms,
  no knowledge of external file formats.
- `crates/sequon-io` — adapters between file formats and the canonical model.
- `crates/sequon-algo` — computation on canonical records.
- `docs/design` — accepted designs and open proposals.

Unknown or unsupported input fails explicitly and reports what was skipped.
Never silently drop qualifiers, features, segments, or provenance.

## Engineering rules

- Stable Rust, edition 2021. `cargo fmt --all -- --check`,
  `cargo clippy --all-targets --all-features -- -D warnings` and
  `cargo test --all-features` must pass before landing.
- `sequon-model` depends on `serde` only; `sequon-algo` must not pull in a
  parser. Sequence format IO is implemented and maintained in this repository:
  GenBank in `sequon-io`; SnapGene `.dna` in its own optional crate or feature,
  import-only. Third-party domain format parsers are format reference and
  development-time comparison only, never a crate or runtime dependency. A
  general-purpose codec or markup tokenizer (for example LZMA or XML) is the
  exception, named in the design note.
- Parsers accept untrusted input: no panics on malformed bytes, no unbounded
  allocation, typed errors that name the field and byte offset.
- Determinism: the same input and the same crate version produce byte-identical
  canonical JSON, including field order. Anything that affects output — parser
  version, algorithm version, restriction enzyme database version — is recorded
  in provenance.
- No `unsafe` without a comment explaining the invariant and a test that covers
  it.
- Public API changes need documentation and a CHANGELOG entry once a version is
  released.

## Licensing and provenance

The workspace declares MIT. Confirm the final license before the first public
release; dual licensing is a one-line change in `Cargo.toml`.

SnapGene `.dna` files are a reverse-engineered binary format with no public
specification, and they carry blocks GenBank cannot represent. When support
lands it is implemented here:

- it lives in its own optional crate or feature under `sequon-io`,
- it is import-only unless a legal review accepts writing,
- the core model compiles and works without it,
- it never becomes the canonical representation.

## Workflow

- `main` is releasable. Develop on short-lived branches.
- Keep commits focused; a change of the canonical model is called out in the
  message.
- Do not add a dependency, a format, or a public type without a design note in
  `docs/design`.
