# Sequon

A Rust framework for DNA and protein sequence processing: file IO, a canonical
JSON data layer, and sequence computation.

Sequon is a library. It has no HTTP service, no daemon and no server endpoints;
consumers embed it and own their storage, transport and authentication.

## Status

Pre-alpha. The canonical model is the first deliverable and is **not decided**.
See [docs/design/canonical-model.md](docs/design/canonical-model.md) before
writing code against it.

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
| `sequon-io` | Adaptors between file formats and the canonical model. |
| `sequon-algo` | Translation, restriction analysis, ligation, assembly, alignment. |

## Non-goals

- No service, daemon, HTTP API or CLI distribution layer in this repository.
- No GUI; graphical clients consume the library.
- No reimplementation of parsers or alignment kernels that a maintained crate
  already provides; wrap and pin them instead.

## Development

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

## License

MIT. See [LICENSE](LICENSE).
