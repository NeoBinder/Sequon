//! Writes or checks the committed synthetic SnapGene fixtures.
//!
//! Usage:
//!
//! ```text
//! cargo run -p sequon-conformance --bin gen-snapgene-fixtures
//! cargo run -p sequon-conformance --bin gen-snapgene-fixtures -- --check
//! ```
//!
//! Without arguments the binary rewrites every file under `testdata/snapgene/`
//! from `sequon_conformance::snapgene_fixtures::all()`. With `--check` it
//! compares the committed bytes instead of writing and exits non-zero on any
//! difference. Regeneration is a deliberate, reviewed change: the normal gate
//! is the determinism test, not this binary.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let check = std::env::args()
        .skip(1)
        .any(|argument| argument == "--check");
    let directory = workspace_root().join("testdata/snapgene");
    if !check {
        if let Err(error) = fs::create_dir_all(&directory) {
            eprintln!("create {}: {error}", directory.display());
            return ExitCode::FAILURE;
        }
    }

    let mut failures = 0_usize;
    for fixture in sequon_conformance::snapgene_fixtures::all() {
        let path = directory.join(fixture.name);
        if check {
            match fs::read(&path) {
                Ok(committed) if committed == fixture.bytes => {
                    println!("ok   {} ({} bytes)", fixture.name, fixture.bytes.len());
                }
                Ok(committed) => {
                    failures += 1;
                    eprintln!(
                        "DIFF {}: committed {} bytes, rebuilt {} bytes",
                        fixture.name,
                        committed.len(),
                        fixture.bytes.len()
                    );
                }
                Err(error) => {
                    failures += 1;
                    eprintln!("MISSING {}: {error}", fixture.name);
                }
            }
        } else {
            if let Err(error) = fs::write(&path, &fixture.bytes) {
                failures += 1;
                eprintln!("write {}: {error}", path.display());
                continue;
            }
            println!("wrote {} ({} bytes)", fixture.name, fixture.bytes.len());
        }
    }

    if failures == 0 {
        ExitCode::SUCCESS
    } else {
        eprintln!("{failures} fixture(s) failed");
        ExitCode::FAILURE
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate lives at <workspace>/crates/sequon-conformance")
        .to_path_buf()
}
