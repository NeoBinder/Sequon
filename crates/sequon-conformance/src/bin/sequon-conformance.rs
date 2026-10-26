//! Command-line entry point for the Sequon conformance harness.
//!
//! Only one subcommand exists today, `compare`, which implements deliverable C9
//! and the plan section 7.5 ground-truth gate:
//!
//! ```text
//! sequon-conformance compare --dna <path.dna> --gb <path.gb> \
//!     [--json <out.json>] [--feature-threshold 0.99]
//! ```
//!
//! Exit codes:
//!
//! - `0`: every threshold passes and no difference was reported;
//! - `1`: at least one threshold failed or a difference was reported
//!   (including informational segment-set differences);
//! - `2`: usage, file, import, multi-record or record-validation error.
//!
//! Argument parsing is manual on purpose: the harness adds no argument-parsing
//! dependency. The comparison itself lives in `sequon_conformance::compare`.

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use sequon_conformance::compare::{self, CompareOptions, CompareReport};
use sequon_io::api::{Format, ImportOptions};
use sequon_io::report::Severity;

/// One-line usage summary.
const USAGE: &str = "\
usage: sequon-conformance compare --dna <path.dna> --gb <path.gb> \
[--json <out.json>] [--feature-threshold <0.0..=1.0>]

  --dna                 SnapGene .dna file (imported with the snapgene feature)
  --gb                  GenBank flat file (one record)
  --json                write the report as JSON to this path
  --feature-threshold   feature Jaccard threshold, default 0.99

exit codes: 0 pass, 1 threshold failure or differences, 2 usage/import/validation error";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match run(&arguments) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run(arguments: &[String]) -> Result<ExitCode, String> {
    let Some((command, rest)) = arguments.split_first() else {
        return Err("missing subcommand".to_owned());
    };
    if command == "--help" || command == "-h" || command == "help" {
        println!("{USAGE}");
        return Ok(ExitCode::SUCCESS);
    }
    if command != "compare" {
        return Err(format!("unknown subcommand '{command}'"));
    }

    let mut dna = None;
    let mut gb = None;
    let mut json = None;
    let mut feature_threshold = None;

    let mut iterator = rest.iter();
    while let Some(argument) = iterator.next() {
        match argument.as_str() {
            "--dna" => dna = Some(next_value(&mut iterator, "--dna")?),
            "--gb" => gb = Some(next_value(&mut iterator, "--gb")?),
            "--json" => json = Some(next_value(&mut iterator, "--json")?),
            "--feature-threshold" => {
                let raw = next_value(&mut iterator, "--feature-threshold")?;
                let value = raw
                    .parse::<f64>()
                    .map_err(|_| format!("invalid --feature-threshold value '{raw}'"))?;
                feature_threshold = Some(CompareOptions::new(value).ok_or_else(|| {
                    format!("--feature-threshold must be in 0.0..=1.0, got {raw}")
                })?);
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(ExitCode::SUCCESS);
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
    }

    let dna_path = dna.ok_or_else(|| "--dna is required".to_owned())?;
    let gb_path = gb.ok_or_else(|| "--gb is required".to_owned())?;
    let options = feature_threshold.unwrap_or_default();

    let dna_record = import_one(Format::Snapgene, Path::new(&dna_path), "dna")?;
    let gb_record = import_one(Format::Genbank, Path::new(&gb_path), "gb")?;
    let report = compare::compare_records(&dna_record, &gb_record, &options);
    print_summary(&report, &dna_path, &gb_path);

    if let Some(json_path) = json {
        write_json(&report, &dna_path, &gb_path, Path::new(&json_path))?;
    }

    Ok(if report.passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn next_value<'a>(
    iterator: &mut std::slice::Iter<'a, String>,
    flag: &str,
) -> Result<String, String> {
    iterator
        .next()
        .cloned()
        .ok_or_else(|| format!("{flag} needs a value"))
}

/// Imports exactly one record and runs the two mandatory checks.
fn import_one(format: Format, path: &Path, label: &str) -> Result<sequon_model::Record, String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("cannot read {label} file {}: {error}", path.display()))?;
    let import_options = ImportOptions {
        format: Some(format),
        ..ImportOptions::default()
    };
    let outcome = match format {
        Format::Snapgene => sequon_io::import_snapgene(&bytes, &import_options),
        Format::Genbank => sequon_io::import_genbank(&bytes, &import_options),
    }
    .map_err(|error| format!("{label} import failed: {error}"))?;

    if outcome.records.len() != 1 {
        return Err(format!(
            "{label} file must contain exactly one record, found {}",
            outcome.records.len()
        ));
    }
    let mut records = outcome.records;
    let record = records.remove(0);
    record
        .validate()
        .map_err(|error| format!("{label} record failed validation: {error}"))?;
    sequon_algo::verify_digests(&record)
        .map_err(|error| format!("{label} record failed digest verification: {error}"))?;

    let error_severity = outcome
        .report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .count();
    if error_severity > 0 {
        eprintln!("warning: {label} import reported {error_severity} error-severity diagnostic(s)");
    }
    Ok(record)
}

fn print_summary(report: &CompareReport, dna_path: &str, gb_path: &str) {
    let verdict = |passed: bool| if passed { "PASS" } else { "FAIL" };
    println!("Sequon conformance compare");
    println!(
        "  dna: {} ({} bp, {})",
        file_label(dna_path),
        report.sequence.dna_length,
        report.topology.dna
    );
    println!(
        "  gb:  {} ({} bp, {})",
        file_label(gb_path),
        report.sequence.gb_length,
        report.topology.gb
    );
    println!(
        "  sequence: {} (lengths {}, normalized {}, sequence digest {}, canonical digest {})",
        verdict(
            report.sequence.lengths_equal
                && report.sequence.normalized_equal
                && report.sequence.sequence_digest_equal
                && report.sequence.canonical_digest_equal != Some(false)
        ),
        report.sequence.lengths_equal,
        report.sequence.normalized_equal,
        report.sequence.sequence_digest_equal,
        match report.sequence.canonical_digest_equal {
            Some(value) => value.to_string(),
            None => "not compared (topology differs)".to_owned(),
        }
    );
    println!(
        "  topology: {} ({} vs {})",
        verdict(report.topology.equal),
        report.topology.dna,
        report.topology.gb
    );
    println!(
        "  features: {} (jaccard {:.4} = {}/{}, threshold {:.4}; segment-set jaccard {:.4} {})",
        verdict(report.features.passed),
        report.features.jaccard,
        report.features.matched,
        report.features.union,
        report.features.threshold,
        report.features.segment_set.jaccard,
        verdict(report.features.segment_set.same)
    );
    for feature in &report.features.missing {
        println!("    missing from GenBank: {feature:?}");
    }
    for feature in &report.features.extra {
        println!("    missing from .dna: {feature:?}");
    }
    println!(
        "  qualifiers: {} ({} compared feature(s), {} difference(s))",
        verdict(report.qualifiers.passed),
        report.qualifiers.compared_features,
        report.qualifiers.differences.len()
    );
    for difference in &report.qualifiers.differences {
        println!("    {:?}", difference);
    }
    println!(
        "  primers: {} ({} vs {} primer_bind)",
        verdict(report.primers.passed),
        report.primers.dna_count,
        report.primers.gb_count
    );
    let threshold_failures = report
        .differences
        .iter()
        .filter(|difference| difference.threshold_failure)
        .count();
    println!(
        "  differences: {} ({} threshold failure(s))",
        report.differences.len(),
        threshold_failures
    );
    for difference in &report.differences {
        println!(
            "    [{}] {}",
            difference.category.as_str(),
            difference.detail
        );
    }
    println!(
        "RESULT: {} (thresholds {}, differences {})",
        verdict(report.passed),
        verdict(report.thresholds.passed),
        verdict(report.differences.is_empty())
    );
}

fn write_json(
    report: &CompareReport,
    dna_path: &str,
    gb_path: &str,
    output: &Path,
) -> Result<(), String> {
    let document = serde_json::json!({
        "schema": "sequon/conformance-compare/1",
        "dna": file_label(dna_path),
        "gb": file_label(gb_path),
        "report": report,
    });
    let mut bytes = serde_json::to_vec_pretty(&document)
        .map_err(|error| format!("cannot serialize comparison report: {error}"))?;
    bytes.push(b'\n');
    fs::write(output, bytes)
        .map_err(|error| format!("cannot write JSON report {}: {error}", output.display()))
}

fn file_label(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "<unnamed>".to_owned())
}
