//! Platform-consumer example: import one sequence file and show what a
//! platform pins beside the canonical payload.
//!
//! The example plays the role of a thin executor in a platform such as
//! NeoAPI-rs. It performs only the duties that consumer owns: read the source
//! bytes, detect and import through the public API, check the report against
//! the returned records and the source digest, run `Record::validate()` and
//! `sequon_algo::verify_digests()` on every record, and print the pin metadata
//! (schema identifiers, embedded schema SHA-256, producer version, source
//! digest) that the platform stores with the payload. The schema bytes and
//! identifiers come from the library constants, so a consumer never has to
//! locate a schema file to pin the exact document its crate release carries. It
//! never re-normalizes a coordinate, re-derives a digest or interprets `extras`.
//!
//! It is deterministic: no clock, no environment lookup, no absolute path in
//! the output, and report sections are ordered by the data itself. A platform
//! would persist the printed pins and the canonical JSON payload; this example
//! prints their sizes instead of writing files.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p sequon-io --example platform-import
//! cargo run -p sequon-io --example platform-import -- testdata/genbank/070-unknown-key.gb
//! cargo run -p sequon-io --features snapgene --example platform-import -- testdata/snapgene/001-linear-uncompressed.dna
//! ```
//!
//! The default input is a committed GenBank fixture, so the example compiles
//! and runs with the default `genbank` feature. SnapGene input needs the
//! `snapgene` feature; the generic `detect`/`import` entry points are the same
//! in both builds.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::process::ExitCode;

use sequon_io::api::{ImportOptions, ImportOutcome};
use sequon_io::json::to_canonical_json_bytes;
use sequon_io::report::{ImportReport, PreservedEncoding, Severity, SourcePosition};
use sequon_io::{Format, ImportError, REPORT_SCHEMA, REPORT_SCHEMA_ID, REPORT_SCHEMA_URN};
use sequon_model::{
    Kind, ProducerKind, SourceFormat, Topology, RECORD_SCHEMA, RECORD_SCHEMA_URN, SCHEMA_ID,
};

/// Committed GenBank fixture used when the caller passes no path.
const DEFAULT_INPUT: &str = "testdata/genbank/010-circular-plasmid.gb";

/// Schema documents whose identifiers and bytes a platform pins.
///
/// The bytes come from the library constants (`sequon_model::RECORD_SCHEMA` and
/// `sequon_io::REPORT_SCHEMA`); the names are kept so the pin report can say
/// which committed document the digest describes.
const RECORD_SCHEMA_FILE: &str = "sequon-record-1.schema.json";
const REPORT_SCHEMA_FILE: &str = "import-report-1.schema.json";

/// Producer name promised for format adapters.
const PARSER_NAME: &str = "sequon-io";

/// Maximum number of dropped or preserved entries printed in full.
const DETAIL_LIMIT: usize = 8;

/// A failure that aborts the example with a message and an exit code.
enum Failure {
    /// The source file could not be read.
    Input(String),
    /// Sequon rejected the input with a typed error.
    Import(ImportError),
    /// A validation, digest, report-consistency or schema-pin check failed.
    Check(String),
}

impl Failure {
    /// Exit code 2 for input/usage problems, 1 for everything else.
    fn exit_code(&self) -> ExitCode {
        match self {
            Failure::Input(_) => ExitCode::from(2),
            Failure::Import(_) | Failure::Check(_) => ExitCode::from(1),
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Input(message) | Failure::Check(message) => formatter.write_str(message),
            Failure::Import(error) => {
                // Malformed and Unsupported already start with the code in
                // their Display; the other variants need it prepended.
                match error {
                    ImportError::Malformed { .. } | ImportError::Unsupported { .. } => {
                        write!(formatter, "{error}")?
                    }
                    ImportError::LimitExceeded { .. }
                    | ImportError::MissingRequired { .. }
                    | ImportError::FormatUnknown { .. } => {
                        write!(formatter, "{}: {error}", error.code())?
                    }
                }
                if let Some(position) = error.position() {
                    write!(formatter, " (at {})", position_text(position))?;
                }
                Ok(())
            }
        }
    }
}

fn main() -> ExitCode {
    let input = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_INPUT.to_owned());

    println!("Sequon platform import example");
    println!("==============================");
    println!();
    println!("input: {}", printable(&input, 160));
    println!(
        "snapgene feature: {}",
        if cfg!(feature = "snapgene") {
            "enabled"
        } else {
            "disabled (build with --features snapgene to read .dna)"
        }
    );

    match run(&input) {
        Ok(records) => {
            println!();
            println!("ok: {records} record(s) imported, validated and digest-verified");
            ExitCode::SUCCESS
        }
        Err(failure) => {
            eprintln!();
            eprintln!("failed: {failure}");
            failure.exit_code()
        }
    }
}

/// Runs the import and prints every consumer-facing result.
///
/// Returns the number of returned records on success.
fn run(input: &str) -> Result<usize, Failure> {
    let source = fs::read(input)
        .map_err(|error| Failure::Input(format!("cannot read input file: {error}")))?;

    // Detection is explicit so the consumer can log the format before parsing;
    // pinning it in the options then avoids detecting the same bytes twice.
    let mut options = ImportOptions::default();
    let detected = sequon_io::detect(&source, &options.limits).map_err(Failure::Import)?;
    options.format = Some(detected);

    println!();
    println!(
        "detected format: {} ({} bytes)",
        format_label(detected),
        source.len()
    );

    // A build without the snapgene feature can still detect a .dna container;
    // say so instead of showing the generic format-detection error.
    if detected == Format::Snapgene && !cfg!(feature = "snapgene") {
        return Err(Failure::Input(
            "input is a SnapGene .dna container but this build has no snapgene feature; rebuild with --features snapgene"
                .to_owned(),
        ));
    }

    // Default options: all records, default limits, no hints, preserve raw
    // blocks. A platform chooses these per deployment; the example keeps the
    // documented defaults so its output is reproducible.
    let outcome = sequon_io::import(&source, &options).map_err(Failure::Import)?;

    // Check before printing: never present unverified output as if it were
    // safe to persist.
    let checks = consumer_checks(&outcome, &source, detected)?;

    print_report_summary(&outcome);
    print_lost_data(&outcome.report);
    print_records(&outcome)?;

    println!();
    println!("consumer checks");
    println!("----------------");
    for line in checks {
        println!("  {line}");
    }

    print_pins(&outcome)?;
    Ok(outcome.records.len())
}

/// Validates the outcome and returns human-readable check lines.
fn consumer_checks(
    outcome: &ImportOutcome,
    source: &[u8],
    detected: Format,
) -> Result<Vec<String>, Failure> {
    check_outcome_consistency(outcome, source, detected)?;

    let mut lines = vec![
        "report summaries agree with the returned records".to_owned(),
        format!(
            "report.source_digest matches the sha256 of the {} input bytes",
            source.len()
        ),
    ];

    for (index, record) in outcome.records.iter().enumerate() {
        record.validate().map_err(|error| {
            Failure::Check(format!(
                "record {index}: Record::validate() failed: {error}"
            ))
        })?;
        sequon_algo::verify_digests(record).map_err(|error| {
            Failure::Check(format!(
                "record {index}: sequon_algo::verify_digests() failed: {error}"
            ))
        })?;
        lines.push(format!(
            "record {index}: Record::validate() and sequon_algo::verify_digests() passed"
        ));
    }

    Ok(lines)
}

/// Checks that the report describes the returned records and the exact input.
///
/// Every check is a consumer duty from docs/design/consumer-integration.md: the
/// payload and the event document must agree before either is persisted. The
/// `report.records` / `outcome.records` length check assumes the default
/// `MultiplePolicy::All`; under `First` the report lists only returned
/// records while diagnostics may still reference non-returned record indices.
fn check_outcome_consistency(
    outcome: &ImportOutcome,
    source: &[u8],
    detected: Format,
) -> Result<(), Failure> {
    let report = &outcome.report;
    let expected_format = match detected {
        Format::Genbank => SourceFormat::Genbank,
        Format::Snapgene => SourceFormat::SnapgeneDna,
    };

    contract(
        report.schema == REPORT_SCHEMA_ID,
        format!(
            "report.schema is '{}', expected '{}'",
            report.schema, REPORT_SCHEMA_ID
        ),
    )?;
    contract(
        report.source_bytes == source.len() as u64,
        format!(
            "report.source_bytes is {}, expected {}",
            report.source_bytes,
            source.len()
        ),
    )?;
    let computed_source_digest = sequon_algo::bytes_digest(source);
    contract(
        report.source_digest == computed_source_digest,
        format!(
            "report.source_digest is {}, computed {}",
            report.source_digest, computed_source_digest
        ),
    )?;
    contract(
        report.source_format == expected_format,
        format!(
            "report.source_format is {}, detection returned {}",
            source_format_label(report.source_format),
            format_label(detected)
        ),
    )?;
    contract(
        report.parser.kind == ProducerKind::Parser,
        format!(
            "report.parser.kind is {}, expected parser",
            producer_kind_label(report.parser.kind)
        ),
    )?;
    contract(
        report.parser.name == PARSER_NAME,
        format!(
            "report.parser.name is '{}', expected '{}'",
            report.parser.name, PARSER_NAME
        ),
    )?;
    contract(
        report.records.len() == outcome.records.len(),
        format!(
            "report lists {} records but the outcome returned {}",
            report.records.len(),
            outcome.records.len()
        ),
    )?;

    for (position, (record, summary)) in outcome.records.iter().zip(&report.records).enumerate() {
        let index = position as u32;
        contract(
            summary.index == index,
            format!(
                "report.records[{position}].index is {}, expected {index}",
                summary.index
            ),
        )?;
        contract(
            summary.name == record.name,
            format!("report.records[{position}].name disagrees with the payload"),
        )?;
        contract(
            summary.kind == record.kind,
            format!("report.records[{position}].kind disagrees with the payload"),
        )?;
        contract(
            summary.length == record.length,
            format!("report.records[{position}].length disagrees with the payload"),
        )?;
        contract(
            summary.sequence_digest == record.digests.sequence,
            format!("report.records[{position}].sequence_digest disagrees with the payload"),
        )?;
        contract(
            summary.canonical_digest == record.digests.canonical,
            format!("report.records[{position}].canonical_digest disagrees with the payload"),
        )?;
        contract(
            record.schema == SCHEMA_ID,
            format!(
                "record {position} schema is '{}', expected '{}'",
                record.schema, SCHEMA_ID
            ),
        )?;
        contract(
            record.provenance.source_digest == report.source_digest,
            format!("record {position} provenance.source_digest disagrees with the report"),
        )?;
        contract(
            record.provenance.source_bytes == report.source_bytes,
            format!("record {position} provenance.source_bytes disagrees with the report"),
        )?;
        contract(
            record.provenance.source_format == report.source_format,
            format!("record {position} provenance.source_format disagrees with the report"),
        )?;
        contract(
            record.provenance.export_version == report.export_version,
            format!("record {position} provenance.export_version disagrees with the report"),
        )?;
        contract(
            record.provenance.producer == report.parser,
            format!("record {position} provenance.producer disagrees with the report parser"),
        )?;
    }

    Ok(())
}

/// Returns Ok when a consumer invariant holds, or a check failure otherwise.
fn contract(condition: bool, message: String) -> Result<(), Failure> {
    if condition {
        Ok(())
    } else {
        Err(Failure::Check(message))
    }
}

/// Prints the report counts a platform stores and alerts on.
fn print_report_summary(outcome: &ImportOutcome) {
    let report = &outcome.report;
    let mut errors = 0_usize;
    let mut warnings = 0_usize;
    let mut infos = 0_usize;
    let mut by_code: BTreeMap<&str, (Severity, usize)> = BTreeMap::new();

    for diagnostic in &report.diagnostics {
        match diagnostic.severity {
            Severity::Error => errors += 1,
            Severity::Warning => warnings += 1,
            Severity::Info => infos += 1,
        }
        // Keep the raw code: a consumer never rejects a report for an unknown
        // code and never parses the human-readable message.
        by_code
            .entry(diagnostic.code.as_str())
            .or_insert((diagnostic.severity, 0))
            .1 += 1;
    }

    println!();
    println!("report summary");
    println!("--------------");
    println!("  records:        {}", outcome.records.len());
    println!(
        "  diagnostics:    {} (error {errors}, warning {warnings}, info {infos})",
        report.diagnostics.len()
    );
    println!(
        "  dropped:        {} (skipped data, fingerprint only)",
        report.dropped.len()
    );
    println!(
        "  preserved:      {} (raw blocks, base64)",
        report.preserved.len()
    );
    println!("  codes_version:  {}", report.codes_version);

    if !by_code.is_empty() {
        let mut entries: Vec<(&str, Severity, usize)> = by_code
            .iter()
            .map(|(code, (severity, count))| (*code, *severity, *count))
            .collect();
        entries.sort_by_key(|(code, severity, _)| (severity_rank(*severity), *code));
        println!("  diagnostics by code:");
        for (code, severity, count) in entries {
            println!("    {:<7} {code} x{count}", severity_label(severity));
        }
    }
}

/// Prints dropped and preserved fingerprints, capped for readability.
fn print_lost_data(report: &ImportReport) {
    if report.dropped.is_empty() && report.preserved.is_empty() {
        return;
    }

    println!();
    println!("lost data (report dropped/preserved entries)");
    println!("-------------------------------------------");

    for entry in report.dropped.iter().take(DETAIL_LIMIT) {
        let mut text = format!("  dropped {}: {}", entry.what, entry.reason);
        if let Some(position) = &entry.position {
            text.push_str(&format!(" at {}", position_text(position)));
        }
        if let Some(length) = entry.length {
            text.push_str(&format!(", {length} bytes"));
        }
        if let Some(digest) = &entry.digest {
            text.push_str(&format!(", {digest}"));
        }
        println!("{text}");
    }
    if report.dropped.len() > DETAIL_LIMIT {
        println!(
            "  dropped: ... {} more entries",
            report.dropped.len() - DETAIL_LIMIT
        );
    }

    for entry in report.preserved.iter().take(DETAIL_LIMIT) {
        let mut text = format!(
            "  preserved {}: {}, {} bytes",
            entry.what,
            preserved_encoding_label(entry.encoding),
            entry.data.len()
        );
        if let Some(length) = entry.length {
            text.push_str(&format!(" (decoded {length})"));
        }
        if let Some(offset) = entry.offset {
            text.push_str(&format!(" at offset {offset}"));
        }
        if let Some(digest) = &entry.digest {
            text.push_str(&format!(", {digest}"));
        }
        println!("{text}");
    }
    if report.preserved.len() > DETAIL_LIMIT {
        println!(
            "  preserved: ... {} more entries",
            report.preserved.len() - DETAIL_LIMIT
        );
    }
}

/// Prints the canonical payload summary for every returned record.
fn print_records(outcome: &ImportOutcome) -> Result<(), Failure> {
    println!();
    println!("records (canonical payload a platform would store)");
    println!("-------------------------------------------------");
    for (index, record) in outcome.records.iter().enumerate() {
        let canonical = to_canonical_json_bytes(record).map_err(|error| {
            Failure::Check(format!(
                "cannot serialize record {index} as canonical JSON: {error}"
            ))
        })?;
        println!("  [{index}] {}", printable(&record.name, 60));
        println!(
            "      kind={} topology={} length={} features={}",
            kind_label(record.kind),
            topology_label(record.topology),
            record.length,
            record.features.len()
        );
        println!("      sequence digest:  {}", record.digests.sequence);
        println!(
            "      canonical digest: {} ({})",
            record.digests.canonical, record.digests.algorithm
        );
        println!("      canonical JSON:   {} bytes", canonical.len());
    }
    Ok(())
}

/// Prints the pin metadata a platform persists beside the payload.
fn print_pins(outcome: &ImportOutcome) -> Result<(), Failure> {
    let report = &outcome.report;
    let record = outcome.records.first().ok_or_else(|| {
        Failure::Check("import returned no record to pin the payload against".to_owned())
    })?;
    let record_schema = schema_pin(RECORD_SCHEMA_FILE, RECORD_SCHEMA, RECORD_SCHEMA_URN)?;
    let report_schema = schema_pin(REPORT_SCHEMA_FILE, REPORT_SCHEMA, REPORT_SCHEMA_URN)?;

    println!();
    println!("platform pins (persist beside the payload)");
    println!("-----------------------------------------");
    pin("record schema:", &record.schema);
    pin("record schema $id:", &record_schema.urn);
    pin(
        "record schema bytes:",
        format!(
            "library-embedded {} {}",
            record_schema.file, record_schema.sha256
        ),
    );
    pin("report schema:", &report.schema);
    pin("report schema $id:", &report_schema.urn);
    pin(
        "report schema bytes:",
        format!(
            "library-embedded {} {}",
            report_schema.file, report_schema.sha256
        ),
    );
    pin(
        "producer:",
        format!(
            "{} {} {}",
            producer_kind_label(report.parser.kind),
            report.parser.name,
            report.parser.version
        ),
    );
    pin("source format:", source_format_label(report.source_format));
    pin("source digest:", &report.source_digest);
    pin("source bytes:", report.source_bytes);
    pin(
        "export version:",
        match report.export_version {
            Some(export_version) => export_version.to_string(),
            None => "none".to_owned(),
        },
    );
    pin("codes version:", &report.codes_version);
    pin("digest algorithm:", &record.digests.algorithm);
    Ok(())
}

/// Prints one left-aligned `label value` row of the pin block.
fn pin(label: &str, value: impl fmt::Display) {
    println!("  {label:<20} {value}");
}

/// A schema document and the identifiers a consumer pins for it.
struct SchemaPin {
    /// File name of the schema document at the repository root.
    file: &'static str,
    /// The schema's stable `$id`.
    urn: String,
    /// SHA-256 of the exact embedded schema bytes.
    sha256: String,
}

/// Computes the pin data for one library-embedded schema.
///
/// The bytes are the constants the library publishes (`sequon_model::RECORD_SCHEMA`
/// and `sequon_io::REPORT_SCHEMA`), not a file the platform has to locate: the
/// consumer pins exactly the schema the loaded crate version carries, and hashes
/// it with the library's own SHA-256 helper. The document is still parsed so the
/// example fails instead of printing a pin if an embedded `$id` and the exported
/// URN constant ever disagree.
fn schema_pin(file: &'static str, bytes: &str, expected_urn: &str) -> Result<SchemaPin, Failure> {
    let document: serde_json::Value = serde_json::from_str(bytes).map_err(|error| {
        Failure::Check(format!("embedded schema {file} is not valid JSON: {error}"))
    })?;
    let urn = document
        .get("$id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Failure::Check(format!("embedded schema {file} has no string $id")))?;
    contract(
        urn == expected_urn,
        format!("embedded schema {file} has $id '{urn}', expected '{expected_urn}'"),
    )?;
    Ok(SchemaPin {
        file,
        urn: urn.to_owned(),
        sha256: sequon_algo::bytes_digest(bytes.as_bytes()).to_string(),
    })
}

/// Formats a source position for human-readable output.
fn position_text(position: &SourcePosition) -> String {
    match position {
        SourcePosition::Genbank { line, column } => match column {
            Some(column) => format!("genbank line {line}, column {column}"),
            None => format!("genbank line {line}"),
        },
        SourcePosition::Snapgene {
            offset,
            packet_type,
            packet_index,
        } => {
            let mut text = format!("snapgene offset {offset}");
            if let Some(packet_type) = packet_type {
                text.push_str(&format!(", packet type {packet_type}"));
            }
            if let Some(packet_index) = packet_index {
                text.push_str(&format!(", packet index {packet_index}"));
            }
            text
        }
    }
}

/// Returns a source-derived string that is safe to print on one terminal line.
///
/// Record names come from untrusted input, so control characters are replaced
/// and long values are truncated; the payload itself is never modified.
fn printable(value: &str, limit: usize) -> String {
    let mut text: String = value
        .chars()
        .take(limit)
        .map(|character| {
            if character.is_control() {
                '?'
            } else {
                character
            }
        })
        .collect();
    if value.chars().count() > limit {
        text.push_str("...");
    }
    text
}

fn format_label(format: Format) -> &'static str {
    match format {
        Format::Genbank => "genbank",
        Format::Snapgene => "snapgene-dna",
    }
}

fn source_format_label(source_format: SourceFormat) -> &'static str {
    match source_format {
        SourceFormat::Genbank => "genbank",
        SourceFormat::SnapgeneDna => "snapgene-dna",
    }
}

fn producer_kind_label(kind: ProducerKind) -> &'static str {
    match kind {
        ProducerKind::Parser => "parser",
        ProducerKind::Algorithm => "algorithm",
        ProducerKind::Manual => "manual",
    }
}

fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Dna => "dna",
        Kind::Rna => "rna",
        Kind::Protein => "protein",
    }
}

fn topology_label(topology: Topology) -> &'static str {
    match topology {
        Topology::Linear => "linear",
        Topology::Circular => "circular",
    }
}

fn severity_label(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
    }
}

fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
    }
}

fn preserved_encoding_label(encoding: PreservedEncoding) -> &'static str {
    match encoding {
        PreservedEncoding::Base64 => "base64",
    }
}
