# Fuzz targets

Three cargo-fuzz targets exercise the untrusted-input paths of `sequon-io`:

| Target | Entry point | What it asserts |
| --- | --- | --- |
| `genbank` | `sequon_io::import_genbank` | no panic; failures are typed `ImportError` values; a returned record passes `Record::validate()` and `sequon_algo::verify_digests()` |
| `dna` | `sequon_io::import_snapgene` | same properties for the SnapGene container reader |
| `location` | `sequon_io::import_genbank` with fuzz bytes embedded as a feature location | no panic in the location grammar; returned records validate |

The targets do not treat a returned error as a failure: a typed error is the
expected handling for malformed bytes. A panic, a hang, or a returned record
that fails validation or digest verification is the finding.

## Layout

This crate is its own workspace (`[workspace]` above) and is deliberately not
a member of the repository workspace, so `cargo test --all-features` at the
root never builds it. `target/`, fuzz corpora and crash artifacts are ignored
by `fuzz/.gitignore` and are never committed.

## Running locally

Nightly Rust and cargo-fuzz are required:

```text
cargo install cargo-fuzz --locked
cargo +nightly fuzz run genbank -- -max_total_time=60
cargo +nightly fuzz run dna -- -max_total_time=60
cargo +nightly fuzz run location -- -max_total_time=60
```

CI runs the same three commands for 60 seconds each on a schedule and in the
nightly job, and the job is allowed to fail so a fuzz finding is visible
without blocking unrelated pull requests. Findings are raised as tasks; they
are never silenced by weakening the assertion in the target.

## What a finding means

- `panicked at ...`: a reader bug. The input belongs in a regression test
  (a fixture or an inline byte builder in `sequon-io/tests/`), not only in the
  fuzz corpus.
- `record failed validation`: the reader produced a record that violates the
  canonical contract; that is a parser bug, not a fuzz harness artifact.
- `digest verification failed`: the reader stored a digest that does not match
  its sequence; treat it as a release blocker.
- a hang: check the configured limits; the reader must fail with
  `LimitExceeded` or a typed error instead of looping.

## Corpus policy

Committed fuzz corpora are not required. A minimized reproducer should become a
deterministic regression test or a committed synthetic fixture under
`testdata/` with its provenance recorded in `testdata/README.md`.
