# Testing and verification

Status: **accepted**, M5 (2026-10-21). This document describes how Sequon is verified, what
runs locally, and what only CI can check. It does not change the accepted
contract; the canonical model, JSON profile and diagnostics documents remain the
source of truth.

Related: [canonical model](canonical-model.md),
[JSON profile](canonical-json-profile.md), [diagnostics](diagnostics.md),
[GenBank adapter](genbank-adapter.md), [SnapGene adapter](snapgene-dna.md),
`testdata/README.md`, plan sections 7 and 7.8.

## 1. Test layers

| Layer | Location | What it proves |
| --- | --- | --- |
| unit | `crates/*/src/**` `#[cfg(test)]` | lexing, location grammar, header mapping, 2-bit decoding, digests, validation |
| adapter integration | `crates/sequon-io/tests/*.rs` | file-level import, typed error codes, limits, determinism, malicious input |
| contract | `tools/validate_contract.py` (CI) and `crates/sequon-conformance/tests/contract_shapes.rs` | schemas compile as 2020-12; every example and golden validates; negative cases are rejected; Rust side checks identities, typed parsing and unknown-field/float rejection |
| fixture determinism | `crates/sequon-conformance/tests/snapgene_fixtures.rs` | committed `.dna` bytes equal the builder output, sha256 matches `testdata/README.md`, container shape is independently walked |
| goldens | `crates/sequon-conformance/tests/snapgene_goldens.rs` | successful fixtures re-import to a valid record, digests verify, canonical JSON byte-equals the golden; failing fixtures return their documented code and have no golden (schema validation of goldens runs in the Python contract gate) |
| dependency policy | `crates/sequon-conformance/tests/dependency_gate.rs` | `Cargo.lock` and manifests contain no banned domain parser; library crates keep their documented runtime dependencies; dev-only crates stay out of the runtime graph |
| performance | `crates/sequon-conformance/tests/bench_regression.rs` | ignored guard against accidental quadratic import behaviour |
| fuzz | `fuzz/fuzz_targets/{genbank,dna,location}.rs` | untrusted input never panics, hangs or yields an invalid record |

Every test that asserts a record calls `Record::validate()` and
`sequon_algo::verify_digests()`. A record that is only structurally present is
not considered verified.

## 2. Local commands

The gates that must pass before landing (plan section 7.8):

```text
python3 -m pip install "jsonschema>=4.23"   # CI installs it before the contract step
python3 tools/validate_contract.py
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test -p sequon-io --no-default-features
cargo test -p sequon-io --features snapgene
cargo doc --no-deps --all-features
cargo check -p sequon-model --target wasm32-unknown-unknown
cargo deny check
```

Scoped runs while iterating:

```text
cargo test -p sequon-conformance --all-features
cargo test -p sequon-io --no-default-features --features snapgene
cargo test -p sequon-conformance --test bench_regression -- --ignored --nocapture
```

Do not run workspace-wide `cargo fmt` or `cargo clippy` while another task is
editing sibling files; use package-scoped commands and the final gates.

## 3. Fixtures and goldens

- Fixtures under `testdata/genbank` and `testdata/snapgene` are synthetic and
  self-authored. The source, licence and sha256 rules are in
  `testdata/README.md`.
- SnapGene fixtures are produced by
  `crates/sequon-conformance/src/dna_builder.rs` through the
  `gen-snapgene-fixtures` binary. The determinism test rebuilds them and
  compares bytes and sha256; regeneration is a reviewed change.
- Goldens under `testdata/expected/<fixture-stem>.json` are canonical JSON
  produced by the committed reader. They are never edited by hand and never
  auto-accepted. SnapGene golden regeneration is an ignored test gated on
  `SEQUON_REGEN_SNAPGENE_GOLDENS=1`; review the diff before committing.
- A fixture whose documented outcome the reader disagrees with is a parser bug
  to escalate. It is not a reason to change the golden or the expectation.
- The local `testdata/corpus-local/` corpus and `testdata/paired/` ground truth
  stay out of git (Q8) and are never required by CI.

## 4. Fuzz

`fuzz/` is its own workspace and is not a member of the repository workspace,
so stable `cargo test` never builds it. Three targets exercise the adapters on
a nightly toolchain:

```text
cargo install cargo-fuzz --locked
cargo +nightly fuzz run genbank -- -max_total_time=60
cargo +nightly fuzz run dna -- -max_total_time=60
cargo +nightly fuzz run location -- -max_total_time=60
```

Targets treat a typed `ImportError` as expected. A panic, a hang, or a returned
record that fails validation or digest verification is a finding. A minimized
reproducer becomes a deterministic regression test or a synthetic fixture; the
fuzz corpus itself is not committed. See `fuzz/README.md`.

## 5. Dependency gate

`deny.toml` carries the machine-readable policy:

- an explicit license allowlist derived from the resolved `Cargo.lock`
  inventory, with narrow exceptions for target-specific crates only;
- bans on third-party domain parsers (`gb-io`, `sgffp`, `biopython`,
  `teselagen`, `bio-parsers`);
- the advisories check.

In addition, CI asserts the shape of the dependency graph:

- `cargo tree -p sequon-io --no-default-features` lists neither `lzma-rs` nor
  `quick-xml`;
- the default GenBank build lists neither either;
- `cargo tree -p sequon-io --features snapgene` lists both;
- `sequon-model` has no hashing, JSON or codec dependency, and
  `sequon-algo` has no parser dependency;
- `Cargo.lock` contains no banned parser name.

The conformance dependency test repeats the manifest and lockfile assertions
locally, so a policy violation fails `cargo test`, not only the CI job.

## 6. Determinism and untrusted input

- Two imports of the same bytes with the same crate version produce identical
  records, diagnostics and canonical JSON.
- Limits reject overruns with a typed error; nothing is truncated or silently
  dropped. Every skipped block appears in `dropped`/`preserved`.
- Parser paths do not panic, allocate unboundedly or read the clock, random
  state or environment.

## 7. What only CI verifies

The following need a hosted runner or tools that are not always installed
locally:

- `cargo deny check` (licenses, bans, advisories; needs `cargo-deny` and the
  advisory database);
- the wasm32 target check (`rustup target add wasm32-unknown-unknown`);
- the nightly cargo-fuzz job (weekly and allowed to fail);
- the GitHub Actions workflow itself, including cache behaviour and job
  permissions.

Everything else in section 2 runs locally. If `cargo deny` is unavailable,
`deny.toml` is still syntax-checked and the conformance dependency test covers
the banned-parser and runtime-dependency rules; the licence and advisory checks
are then CI-only.

## 8. Reporting

A milestone report records: commands run, pass/fail, the observed performance
numbers, any fixture the reader disagreed with, and anything deferred to CI.
Differences in goldens or fixtures are listed explicitly; an unexplained
difference blocks sign-off.
