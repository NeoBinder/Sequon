//! Typed import errors.
//!
//! Errors are returned when no usable record can be produced. Warnings and
//! informational events are carried by the import report instead. Every variant
//! either names a stable code from docs/design/diagnostics.md or is a
//! detection failure that has no report.

use std::error::Error;
use std::fmt;

use crate::report::SourcePosition;

/// A record field that a format did not provide and no hint supplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RequiredField {
    /// The record name.
    Name,
    /// The molecule kind.
    Kind,
    /// The sequence topology.
    Topology,
}

impl RequiredField {
    /// Returns the canonical field name used in diagnostics and messages.
    pub fn as_str(self) -> &'static str {
        match self {
            RequiredField::Name => "name",
            RequiredField::Kind => "kind",
            RequiredField::Topology => "topology",
        }
    }
}

impl fmt::Display for RequiredField {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A typed failure that prevents producing a usable import outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// A configured limit was exceeded. The limit is never truncated.
    LimitExceeded {
        /// Name of the configured limit, for example "max_input_bytes".
        limit: &'static str,
        /// Configured value.
        limit_value: u64,
        /// Observed value.
        actual: u64,
        /// Where the overrun was detected, when known.
        position: Option<SourcePosition>,
    },
    /// The input is malformed. The code is a stable error code.
    Malformed {
        /// Stable code from docs/design/diagnostics.md.
        code: &'static str,
        /// Human-readable explanation; never contains sequence content.
        message: String,
        /// Where the problem was detected, when known.
        position: Option<SourcePosition>,
    },
    /// The input is well-formed but uses a construct version 1 does not support.
    Unsupported {
        /// Stable code from docs/design/diagnostics.md.
        code: &'static str,
        /// Human-readable explanation.
        message: String,
        /// Where the construct was found, when known.
        position: Option<SourcePosition>,
    },
    /// A required record field has no source and no hint.
    MissingRequired {
        /// The missing field.
        field: RequiredField,
        /// Where the record starts, when known.
        position: Option<SourcePosition>,
    },
    /// Format detection failed.
    FormatUnknown {
        /// What was inspected, for example the first bytes or the extension.
        hint: String,
    },
}

impl ImportError {
    /// Returns the stable code carried by this error.
    pub fn code(&self) -> &'static str {
        match self {
            ImportError::LimitExceeded { .. } => "LIMIT_EXCEEDED",
            ImportError::Malformed { code, .. } | ImportError::Unsupported { code, .. } => code,
            ImportError::MissingRequired { .. } => "RECORD_REQUIRED_FIELD_MISSING",
            ImportError::FormatUnknown { .. } => "FORMAT_UNKNOWN",
        }
    }

    /// Returns the source position carried by this error, when any.
    pub fn position(&self) -> Option<&SourcePosition> {
        match self {
            ImportError::LimitExceeded { position, .. }
            | ImportError::Malformed { position, .. }
            | ImportError::Unsupported { position, .. }
            | ImportError::MissingRequired { position, .. } => position.as_ref(),
            ImportError::FormatUnknown { .. } => None,
        }
    }
}

impl fmt::Display for ImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::LimitExceeded {
                limit,
                limit_value,
                actual,
                ..
            } => write!(
                formatter,
                "{limit} exceeded: configured {limit_value}, observed {actual}"
            ),
            ImportError::Malformed { code, message, .. } => {
                write!(formatter, "{code}: {message}")
            }
            ImportError::Unsupported { code, message, .. } => {
                write!(formatter, "{code}: {message}")
            }
            ImportError::MissingRequired { field, .. } => {
                write!(formatter, "required record field '{field}' has no source")
            }
            ImportError::FormatUnknown { hint } => {
                write!(formatter, "input format is not recognized: {hint}")
            }
        }
    }
}

impl Error for ImportError {}
