//! Configurable limits for untrusted input.
//!
//! Defaults are fixed by docs/design/development-plan.md section 3.6 and the
//! accepted adapter design notes. Every adapter checks a limit before it
//! allocates or copies, and exceeding one is a typed ImportError::LimitExceeded
//! with the limit name, the configured value, the observed value and a position
//! when one is known. Limits are never enforced by truncation.

/// Default and configurable limits for parsing untrusted input.
///
/// The fields are public so a caller can tighten them for a constrained
/// deployment. Values are byte counts or element counts as named by the field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// Maximum size of the whole input in bytes.
    pub max_input_bytes: u64,
    /// Maximum number of records in one multi-record input.
    pub max_records: u64,
    /// Maximum length of one sequence in bases or residues.
    pub max_sequence_len: u64,
    /// Maximum number of features in one record.
    pub max_features: u64,
    /// Maximum number of segments in one join/order location.
    pub max_segments_per_feature: u64,
    /// Maximum size of one qualifier value in bytes after unescaping.
    pub max_qualifier_value_bytes: u64,
    /// Maximum accumulated size of a GenBank header in bytes.
    pub max_header_bytes: u64,
    /// Maximum nesting depth of a location expression.
    pub max_location_depth: u64,
    /// Maximum number of TLV packets in a SnapGene container.
    pub max_snapgene_packets: u64,
    /// Maximum number of bytes an LZMA block may decode to.
    pub max_lzma_output_bytes: u64,
    /// Maximum decompressed-to-compressed ratio for an LZMA block.
    pub max_lzma_ratio: u64,
    /// Maximum size of the LZMA dictionary the decoder may allocate.
    pub max_lzma_dict_bytes: u64,
    /// Maximum XML nesting depth.
    pub max_xml_depth: u64,
    /// Maximum size of one XML element's text in bytes.
    pub max_xml_element_bytes: u64,
    /// Maximum number of XML nodes (elements plus attributes) in one block.
    pub max_xml_nodes: u64,
    /// Maximum total decoded XML bytes across the blocks of one import.
    pub max_xml_bytes: u64,
    /// Maximum total bytes of raw blocks preserved in an import report.
    pub max_preserved_block_bytes: u64,
}

impl Limits {
    /// Returns the documented defaults.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_input_bytes: 256 * 1024 * 1024,
            max_records: 64,
            max_sequence_len: 16 * 1024 * 1024,
            max_features: 100_000,
            max_segments_per_feature: 10_000,
            max_qualifier_value_bytes: 1024 * 1024,
            max_header_bytes: 4 * 1024 * 1024,
            max_location_depth: 32,
            max_snapgene_packets: 65_536,
            max_lzma_output_bytes: 64 * 1024 * 1024,
            max_lzma_ratio: 200,
            max_lzma_dict_bytes: 8 * 1024 * 1024,
            max_xml_depth: 64,
            max_xml_element_bytes: 1024 * 1024,
            max_xml_nodes: 1_000_000,
            max_xml_bytes: 64 * 1024 * 1024,
            max_preserved_block_bytes: 8 * 1024 * 1024,
        }
    }
}
