# Sequon

A Rust framework for DNA and protein sequence processing: file IO, a canonical
JSON data layer, and sequence computation.

Sequon is a library. It has no HTTP service, no daemon and no server endpoints;
consumers embed it and own their storage, transport and authentication.

## Status

Pre-alpha. The version-1 canonical model, JSON Schemas and diagnostic
vocabulary are accepted and implemented. GenBank read/write is available by
default; SnapGene `.dna` import (read-only) is available behind the `snapgene`
feature. Every committed fixture and golden is checked by the conformance
crate. See [docs/design/README.md](docs/design/README.md) for the contracts.

## Design principles

- **One canonical representation.** File formats are adapters; the model is the
  truth. GenBank, FASTA and SnapGene are ways in and out, not storage.
- **JSON on the wire and on disk.** Serialized records use the `.json`
  extension. No custom binary container, no custom extension.
- **Lossless where possible, explicit where not.** Anything the model cannot
  represent is reported as a warning with its source position, never dropped
  silently.
- **Deterministic.** Same input and same version produce byte-identical output,
  including provenance (parser, algorithm and database versions).
- **Untrusted input.** Parsers are written for files that may be malformed or
  hostile.

## Crates

| Crate | Role |
| --- | --- |
| `sequon-model` | Canonical types and invariants. Depends on `serde` only. |
| `sequon-io` | Adapters between file formats and the canonical model. |
| `sequon-algo` | Normalization, reverse complement, minimum rotation and `sequon-digest/1` digests. Translation and analysis come later. |

## Scope and non-goals

Embedded in a process as a library, consuming and producing canonical records.
Not in this repository: a service, daemon, HTTP API or CLI distribution layer; a
GUI (graphical clients consume the library); alignment kernels that a maintained
crate already provides (wrap and pin them). Sequence format IO is not delegated:
it is implemented here, never wrapped from an external parser crate or runtime.

## Quick start

```rust
use sequon_io::{ImportOptions, import};

let outcome = import(bytes, &ImportOptions::default())?;
for record in &outcome.records {
    record.validate()?;
    sequon_algo::verify_digests(record)?;
}
```

`ImportOutcome` also carries the `sequon/import-report/1` event document with
diagnostics, dropped data and preserved blocks. `write_genbank` serializes a
DNA record back to GenBank text.

## Development

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

MIT licensed; see [LICENSE](LICENSE).
