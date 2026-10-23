#![no_main]

//! Fuzz target: GenBank feature-location grammar.
//!
//! The fuzz bytes are embedded as the location of one feature in an otherwise
//! valid record, so the location parser is reached directly. Any panic, hang or
//! unvalidated record is a finding; a typed `ImportError` is acceptable.

use libfuzzer_sys::fuzz_target;
use sequon_io::api::ImportOptions;

/// Builds a valid one-record GenBank document whose single feature location is
/// `location`.
fn record_with_location(location: &str) -> Vec<u8> {
    let sequence = "ACGT".repeat(25);
    let mut origin = String::new();
    for (line, chunk) in sequence.as_bytes().chunks(60).enumerate() {
        origin.push_str(&format!("{:>9} ", 1 + line * 60));
        origin.push_str(std::str::from_utf8(chunk).expect("ASCII sequence"));
        origin.push('\n');
    }
    format!(
        "LOCUS       FUZZLOCUS        100 bp    DNA     linear   SYN 01-JAN-2000\n\
         DEFINITION  fuzz target.\n\
         ACCESSION   FUZZ\n\
         FEATURES             Location/Qualifiers\n\
         \x20    misc_feature    {location}\n\
         \x20                    /note=\"fuzz\"\n\
         ORIGIN\n{origin}//\n"
    )
    .into_bytes()
}

fuzz_target!(|data: &[u8]| {
    if data.len() > 4096 {
        return;
    }
    let raw = String::from_utf8_lossy(data);
    let location: String = raw
        .chars()
        .filter(|character| *character != '\n' && *character != '\r')
        .take(512)
        .collect();
    let location = if location.trim().is_empty() {
        "1..100"
    } else {
        location.as_str()
    };
    let bytes = record_with_location(location);
    let options = ImportOptions::default();
    if let Ok(outcome) = sequon_io::import_genbank(&bytes, &options) {
        for record in &outcome.records {
            record
                .validate()
                .expect("reader returned a record that fails Record::validate()");
            sequon_algo::verify_digests(record)
                .expect("reader returned a record with inconsistent digests");
        }
    }
});
