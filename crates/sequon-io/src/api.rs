//! Shared import and export API types.
//!
//! The shapes are fixed by the public API decision (S9) in the task plan and by
//! docs/design/canonical-model.md. Adapters implement the functions; these types
//! are what callers construct and receive.

use std::error::Error;
use std::fmt;

use sequon_model::{Kind, Record, Topology};

use crate::error::ImportError;
use crate::limits::Limits;
use crate::report::{Diagnostic, ImportReport};

/// A supported input format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    /// GenBank flat file text.
    Genbank,
    /// SnapGene binary container.
    Snapgene,
}

/// How a multi-record input is handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MultiplePolicy {
    /// Return only the first record; parse the rest for diagnostics.
    First,
    /// Return every record.
    #[default]
    All,
    /// Reject an input that contains more than one record.
    Reject,
}

/// Options for one import.
#[derive(Debug, Clone)]
pub struct ImportOptions {
    /// Skip content detection when set.
    pub format: Option<Format>,
    /// Configured limits for untrusted input.
    pub limits: Limits,
    /// Multi-record policy.
    pub multiple: MultiplePolicy,
    /// Caller-supplied name, used only when the format provides none.
    pub name_hint: Option<String>,
    /// Caller-supplied kind, used only when the format provides none.
    pub kind_hint: Option<Kind>,
    /// Caller-supplied topology, used only when the format provides none.
    pub topology_hint: Option<Topology>,
    /// Keep raw format blocks that the model cannot express.
    pub preserve_unhandled_blocks: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            format: None,
            limits: Limits::default(),
            multiple: MultiplePolicy::default(),
            name_hint: None,
            kind_hint: None,
            topology_hint: None,
            preserve_unhandled_blocks: true,
        }
    }
}

/// Result of a successful import.
#[derive(Debug, Clone)]
pub struct ImportOutcome {
    /// Records returned by the import, in output order.
    pub records: Vec<Record>,
    /// Event document describing the import.
    pub report: ImportReport,
}

/// Options for writing GenBank.
#[derive(Debug, Clone)]
pub struct WriteOptions {
    /// LOCUS date in DD-MON-YYYY form; None uses the fixed placeholder.
    pub date: Option<String>,
    /// Preferred line width; 80 by default.
    pub line_width: usize,
    /// Restore header fields preserved in extras.genbank.header.
    pub include_header_extras: bool,
}

impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            date: None,
            line_width: 80,
            include_header_extras: true,
        }
    }
}

/// Result of a successful write.
///
/// Non-fatal writer diagnostics travel here; a failure that prevents producing
/// bytes is returned as a WriteError instead.
#[derive(Debug, Clone)]
pub struct WriteOutcome {
    /// The serialized bytes.
    pub bytes: Vec<u8>,
    /// Warnings and information produced while writing.
    pub diagnostics: Vec<Diagnostic>,
}

/// A typed failure that prevents writing a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteError {
    /// The record kind has no version 1 writer.
    UnsupportedKind {
        /// The unsupported kind.
        kind: Kind,
    },
    /// The record is structurally invalid for writing.
    InvalidRecord {
        /// What is wrong with the record.
        message: String,
    },
    /// The record cannot be expressed in the target format.
    UnsupportedFeature {
        /// What cannot be expressed.
        message: String,
    },
}

impl fmt::Display for WriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WriteError::UnsupportedKind { kind } => {
                write!(formatter, "no writer for kind {kind:?}")
            }
            WriteError::InvalidRecord { message } => write!(formatter, "invalid record: {message}"),
            WriteError::UnsupportedFeature { message } => {
                write!(formatter, "record cannot be written: {message}")
            }
        }
    }
}

impl Error for WriteError {}

/// Error type used by the public import entry points.
pub type ImportResult = Result<ImportOutcome, ImportError>;
