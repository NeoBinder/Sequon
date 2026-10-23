//! Content-based format detection.
//!
//! Detection reads the input bytes, not the file extension: a .dna container and
//! a GenBank flat file are recognized by their own markers. The extension is at
//! most a hint at the caller's boundary.

use crate::api::{Format, ImportOptions, ImportResult};
use crate::error::ImportError;
use crate::limits::Limits;

/// The SnapGene cookie marker: packet type 9 followed by the ASCII name.
const SNAPGENE_MAGIC: &[u8] = b"SnapGene";

/// Detects the format of an input buffer.
///
/// Returns FormatUnknown when neither marker is present. The input-size limit is
/// checked here so every entry point rejects oversized input the same way.
pub fn detect(bytes: &[u8], limits: &Limits) -> Result<Format, ImportError> {
    if bytes.len() as u64 > limits.max_input_bytes {
        return Err(ImportError::LimitExceeded {
            limit: "max_input_bytes",
            limit_value: limits.max_input_bytes,
            actual: bytes.len() as u64,
            position: None,
        });
    }
    if looks_like_snapgene(bytes) {
        return Ok(Format::Snapgene);
    }
    if looks_like_genbank(bytes) {
        return Ok(Format::Genbank);
    }
    Err(ImportError::FormatUnknown {
        hint: describe(bytes),
    })
}

/// Imports an input using the requested format or detection.
pub(crate) fn import(bytes: &[u8], options: &ImportOptions) -> ImportResult {
    let format = match options.format {
        Some(format) => format,
        None => detect(bytes, &options.limits)?,
    };
    match format {
        Format::Genbank => import_genbank_feature(bytes, options),
        Format::Snapgene => import_snapgene_feature(bytes, options),
    }
}

#[cfg(feature = "genbank")]
fn import_genbank_feature(bytes: &[u8], options: &ImportOptions) -> ImportResult {
    crate::genbank::import(bytes, options)
}

#[cfg(not(feature = "genbank"))]
fn import_genbank_feature(_bytes: &[u8], _options: &ImportOptions) -> ImportResult {
    Err(ImportError::FormatUnknown {
        hint: "GenBank support is not enabled in this build (feature 'genbank')".to_string(),
    })
}

#[cfg(feature = "snapgene")]
fn import_snapgene_feature(bytes: &[u8], options: &ImportOptions) -> ImportResult {
    crate::snapgene::import(bytes, options)
}

#[cfg(not(feature = "snapgene"))]
fn import_snapgene_feature(_bytes: &[u8], _options: &ImportOptions) -> ImportResult {
    Err(ImportError::FormatUnknown {
        hint: "SnapGene support is not enabled in this build (feature 'snapgene')".to_string(),
    })
}

fn looks_like_snapgene(bytes: &[u8]) -> bool {
    bytes.len() >= 13
        && bytes[0] == 0x09
        && &bytes[5..13] == SNAPGENE_MAGIC
        && bytes[1..5].iter().any(|byte| *byte != 0)
}

fn looks_like_genbank(bytes: &[u8]) -> bool {
    for line in bytes.split(|byte| *byte == b'\n').take(64) {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.starts_with(b"LOCUS ") || line == b"LOCUS" {
            return true;
        }
        if line.iter().any(|byte| !byte.is_ascii_whitespace()) {
            return false;
        }
    }
    false
}

fn describe(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "input is empty".to_string();
    }
    let printable: String = bytes
        .iter()
        .take(16)
        .map(|byte| {
            if byte.is_ascii_graphic() || *byte == b' ' {
                char::from(*byte)
            } else {
                '?'
            }
        })
        .collect();
    format!("first {} bytes: {printable}", bytes.len().min(16))
}
