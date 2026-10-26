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
| contract | `crates/sequon-conformance/tests/schema_validation.rs` | schemas compile; every `schemas/examples/*.json` validates; negative cases are rejected |
| fixture determinism | `crates/sequon-conformance/tests/snapgene_fixtures.rs` | committed `.dna` bytes equal the builder output, sha256 matches `testdata/README.md`, container shape is independently walked |
| goldens | `crates/sequon-conformance/tests/snapgene_goldens.rs` | successful fixtures re-import to a valid record, digests verify, canonical JSON byte-equals the golden, goldens validate against the record schema; failing fixtures return their documented code and have no golden |
| cross-format compare (C9) | `crates/sequon-conformance/src/compare.rs`, `tests/compare.rs`, `src/bin/sequon-conformance.rs` | `.dna` versus GenBank semantic comparison: synthetic writer round-trip must pass every threshold; negative cases fail their category; CLI exit codes 0/1/2; local pET-28a pair gated on `testdata/corpus-local/` |
| dependency policy | `crates/sequon-conformance/tests/dependency_gate.rs` | `Cargo.lock` and manifests contain no banned domain parser; library crates keep their documented runtime dependencies; dev-only crates stay out of the runtime graph |
| performance | `crates/sequon-conformance/tests/bench_regression.rs` | ignored guard against accidental quadratic import behaviour |
| fuzz | `fuzz/fuzz_targets/{genbank,dna,location}.rs` | untrusted input never panics, hangs or yields an invalid record |

Every test that asserts a record calls `Record::validate()` and
`sequon_algo::verify_digests()`. A record that is only structurally present is
not considered verified.

## 2. Local commands

The gates that must pass before landing (plan section 7.8):

```text
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

### 3.1 Cross-format compare tool (C9)

```text
cargo run -p sequon-conformance --bin sequon-conformance -- compare \
  --dna <path.dna> --gb <path.gb> [--json <out.json>] [--feature-threshold 0.99]
```

The tool imports exactly one record per file with `sequon-io`, runs
`Record::validate()` and `sequon_algo::verify_digests()`, and reports:

- sequence: lengths, normalized equality, direction-sensitive sequence digest
  equality, and canonical digest equality when the topology matches;
- topology equality;
- features: Jaccard over the multiset of `(key, first.start, last.end,
  strand)` with origin-resolved segments, the documented threshold (default
  0.99), missing / extra lists, duplicate-count differences, and a
  segment-set-aware secondary Jaccard that is informational;
- qualifiers: per matched feature, multiset differences after documented
  normalization (trim, quote-escape reversal, outer quotes, whitespace
  collapse, case-folded values);
- primers: `primer_bind` counts, 100% required.

Exit codes are `0` (all thresholds pass and no differences), `1` (threshold
failure or any difference, including informational ones) and `2` (usage,
file, import, multi-record or validation error). `--json` writes the report as
JSON marked `sequon/conformance-compare/1`; it is a dev-only report, not a
contract schema (it contains floating-point numbers, so the canonical JSON
profile does not apply), and it carries the report and input file names only,
never machine paths.

Tests in `crates/sequon-conformance/tests/compare.rs` build a synthetic
`.dna`, import it, write GenBank, import that and require a 100% pass; they
mutate one base, the topology, the feature set and a qualifier to require the
documented failures, and cover duplicate features (multiset semantics), a
quoted qualifier round-trip, primer counts, empty feature sets and an
origin-crossing circular span against its split `join`; and they run the binary
through `std::process::Command` for the exit codes. A local pET-28a(+) pair is compared when
`testdata/corpus-local/pet28a-pair/` exists: sequence, topology and canonical
digests must match, and the feature gate must fail because the GenBank file is a
patent record with two features rather than a SnapGene Save-As export
(`testdata/README.md` section 8.4).

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
