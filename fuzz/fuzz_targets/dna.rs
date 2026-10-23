#![no_main]

//! Fuzz target: SnapGene .dna import.
//!
//! Any panic, hang or unvalidated record is a finding. A typed `ImportError`
//! is the expected result for malformed input and is not a failure.

use libfuzzer_sys::fuzz_target;
use sequon_io::api::ImportOptions;

fuzz_target!(|data: &[u8]| {
    if data.len() > 1024 * 1024 {
        return;
    }
    let options = ImportOptions::default();
    if let Ok(outcome) = sequon_io::import_snapgene(data, &options) {
        for record in &outcome.records {
            record
                .validate()
                .expect("reader returned a record that fails Record::validate()");
            sequon_algo::verify_digests(record)
                .expect("reader returned a record with inconsistent digests");
        }
    }
});
