//! Performance regression guards for the import path.
//!
//! Ignored by default because wall-clock numbers are machine dependent. Run
//! with `cargo test -p sequon-conformance --test bench_regression -- --ignored --nocapture`.
//! The budgets are deliberately generous: this is a regression guard for
//! accidental quadratic behaviour, not a benchmark suite. Plan section 7.7
//! records the product targets (5 kb plasmid < 10 ms, 1 Mb GenBank < 500 ms on
//! a developer machine); CI debug builds are slower, so the assertions use a
//! wider margin and print the observed numbers for the record.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sequon_io::api::ImportOptions;

/// Builds a deterministic 5 000 bp circular GenBank record.
fn synthetic_five_kb_genbank() -> Vec<u8> {
    let sequence = "ACGT".repeat(1250);
    let mut origin = String::new();
    for (line, chunk) in sequence.as_bytes().chunks(60).enumerate() {
        origin.push_str(&format!("{:>9} ", 1 + line * 60));
        origin.push_str(std::str::from_utf8(chunk).expect("ASCII sequence"));
        origin.push('\n');
    }
    format!(
        "LOCUS       {:<24}{} bp    DNA     circular SYN 01-JAN-2000\n\
         DEFINITION  synthetic 5 kb benchmark plasmid.\n\
         ACCESSION   BENCH5KB\n\
         FEATURES             Location/Qualifiers\n\
         \x20    CDS             100..900\n\
         \x20                    /gene=\"bench\"\n\
         \x20                    /product=\"synthetic benchmark protein\"\n\
         ORIGIN\n{origin}//\n",
        "BENCH5KB", 5000
    )
    .into_bytes()
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate lives at <workspace>/crates/sequon-conformance")
        .to_path_buf()
}

fn best_of(iterations: usize, mut run: impl FnMut()) -> Duration {
    let mut best = Duration::MAX;
    for _ in 0..iterations {
        let started = Instant::now();
        run();
        best = best.min(started.elapsed());
    }
    best
}

fn import_and_assert(bytes: &[u8], label: &str) {
    let options = ImportOptions::default();
    let outcome = sequon_io::import_genbank(bytes, &options)
        .unwrap_or_else(|error| panic!("{label} must import: {error}"));
    assert_eq!(outcome.records.len(), 1, "{label} record count");
    let record = &outcome.records[0];
    record
        .validate()
        .unwrap_or_else(|error| panic!("{label} record invalid: {error}"));
    sequon_algo::verify_digests(record).unwrap_or_else(|error| panic!("{label} digests: {error}"));
}

#[test]
#[ignore = "performance regression guard; run with --ignored --nocapture"]
fn import_time_regression_5kb_and_1mb() {
    let five_kb = synthetic_five_kb_genbank();
    import_and_assert(&five_kb, "5 kb synthetic plasmid");
    let five_ms = best_of(5, || import_and_assert(&five_kb, "5 kb synthetic plasmid"));
    println!("5 kb synthetic plasmid import: {five_ms:?}");
    assert!(
        five_ms < Duration::from_millis(500),
        "5 kb import regression: {five_ms:?} exceeds 500 ms"
    );

    let large_path = workspace_root().join("testdata/genbank/150-large-1mb.gb");
    let large = std::fs::read(&large_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", large_path.display()));
    import_and_assert(&large, "1 Mb GenBank fixture");
    let large_time = best_of(3, || import_and_assert(&large, "1 Mb GenBank fixture"));
    println!("1 Mb GenBank fixture import: {large_time:?}");
    assert!(
        large_time < Duration::from_secs(10),
        "1 Mb import regression: {large_time:?} exceeds 10 s"
    );
}
