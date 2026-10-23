//! GenBank location grammar and coordinate conversion.
//!
//! The grammar is parsed into a raw 1-based closed form first, because the
//! sequence length needed for out-of-range checks and circular normalization is
//! only known after ORIGIN has been read. Conversion to 0-based half-open
//! segments happens once, after the sequence is available.
//!
//! On a circular record a range that crosses the origin stays one unwrapped
//! segment whose `end` is greater than the sequence length (the 2026-10-23
//! model revision). An explicit `join` or `order` keeps its own segments: the
//! source syntax is preserved, never split and never merged.

use sequon_model::{Location, Segment, Topology, OPERATOR_JOIN, OPERATOR_ORDER, OPERATOR_SPAN};

use crate::error::ImportError;
use crate::limits::Limits;
use crate::report::SourcePosition;

/// One raw segment in GenBank's 1-based closed coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawSegment {
    /// 1-based first position.
    pub start1: u64,
    /// 1-based last position.
    pub end1: u64,
    /// True when the source marked the start with '<'.
    pub start_partial: bool,
    /// True when the source marked the end with '>'.
    pub end_partial: bool,
    /// True for a zero-length between-positions site (n^m).
    pub zero_len: bool,
}

/// A parsed location before length validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawLocation {
    /// span, join or order.
    pub operator: &'static str,
    /// 1 or -1; GenBank has no unstranded spelling.
    pub strand: i8,
    /// Segments in written order.
    pub segments: Vec<RawSegment>,
}

/// A location that cannot be parsed, or a limit that aborts the import.
#[derive(Debug)]
pub(crate) enum LocationError {
    /// Syntactic error: the record is unusable (GB_LOCATION_MALFORMED).
    Malformed { message: String, column: u32 },
    /// A construct version 1 cannot represent; the feature is skipped.
    Unsupported { message: String, _column: u32 },
    /// A configured limit was exceeded.
    Fatal(ImportError),
}

/// Parses a location expression.
///
/// `start_col` is the 1-based column of the first character of `text`, used
/// for line + column positions on malformed input.
pub(crate) fn parse(
    text: &str,
    line: u32,
    start_col: u32,
    limits: &Limits,
) -> Result<RawLocation, LocationError> {
    let mut parser = Parser {
        text,
        pos: 0,
        line,
        start_col,
        limits,
        segments_seen: 0,
    };
    parser.parse()
}

/// Converts a parsed location to canonical coordinates.
///
/// Returns Err(()) when a coordinate cannot be represented: linear
/// out-of-range, circular span longer than the sequence, or a between site
/// outside its legal range. The caller skips the whole feature in that case.
///
/// A circular range is stored unwrapped: `{5899,6100}` on a 6000 bp record is
/// one `span` segment, not two `join` pieces. An explicit `join`/`order` keeps
/// its segments in written order; each segment is normalized by the same rule
/// and no two segments are merged.
pub(crate) fn normalize(
    raw: &RawLocation,
    topology: Topology,
    length: u64,
) -> Result<Location, ()> {
    let mut segments = Vec::with_capacity(raw.segments.len());
    for segment in &raw.segments {
        if segment.zero_len {
            let site = segment.start1;
            let valid = match topology {
                Topology::Linear => site < length,
                Topology::Circular => site <= length,
            };
            if !valid {
                return Err(());
            }
            segments.push(Segment::with_boundaries(site, site, false, false));
            continue;
        }

        let start0 = segment.start1 - 1;
        let end0 = segment.end1;
        match topology {
            Topology::Linear => {
                if end0 > length || start0 >= length {
                    return Err(());
                }
                segments.push(Segment::with_boundaries(
                    start0,
                    end0,
                    segment.start_partial,
                    segment.end_partial,
                ));
            }
            Topology::Circular => {
                let span = end0 - start0;
                if span > length || length == 0 {
                    return Err(());
                }
                // Keep the source span unwrapped instead of splitting it at the
                // origin: one segment may end past `length` by at most one
                // length. `Segment::resolved` maps it back on access.
                let start = start0 % length;
                segments.push(Segment::with_boundaries(
                    start,
                    start + span,
                    segment.start_partial,
                    segment.end_partial,
                ));
            }
        }
    }

    // Join and order are carried verbatim. A span always normalizes to exactly
    // one segment, so no operator rewrite is needed here.
    Ok(Location {
        operator: raw.operator.to_string(),
        strand: raw.strand,
        segments,
    })
}

/// A node in the parsed location grammar.
enum Node {
    Segment(RawSegment),
    Complement(Box<Node>),
    Join(Vec<Node>),
    Order(Vec<Node>),
}

struct Parser<'a> {
    text: &'a str,
    pos: usize,
    line: u32,
    start_col: u32,
    limits: &'a Limits,
    segments_seen: u64,
}

impl Parser<'_> {
    fn parse(&mut self) -> Result<RawLocation, LocationError> {
        if self.text.contains(':') {
            return Err(LocationError::Unsupported {
                message: "remote locations are not supported in version 1".to_string(),
                _column: self.column(),
            });
        }
        let node = self.parse_node(1)?;
        self.skip_whitespace();
        if self.pos < self.text.len() {
            return Err(self.malformed("unexpected characters after the location"));
        }
        let raw = flatten(node)?;
        if raw.segments.len() as u64 > self.limits.max_segments_per_feature {
            return Err(LocationError::Fatal(ImportError::LimitExceeded {
                limit: "max_segments_per_feature",
                limit_value: self.limits.max_segments_per_feature,
                actual: raw.segments.len() as u64,
                position: Some(self.position()),
            }));
        }
        Ok(raw)
    }

    fn parse_node(&mut self, depth: u32) -> Result<Node, LocationError> {
        if u64::from(depth) > self.limits.max_location_depth {
            return Err(LocationError::Fatal(ImportError::LimitExceeded {
                limit: "max_location_depth",
                limit_value: self.limits.max_location_depth,
                actual: u64::from(depth),
                position: Some(self.position()),
            }));
        }

        self.skip_whitespace();
        if self.pos >= self.text.len() {
            return Err(self.malformed("empty location"));
        }

        let saved = self.pos;
        if let Some(identifier) = self.try_identifier() {
            self.skip_whitespace();
            let has_paren = self.peek() == Some('(');
            if is_unsupported_operator(&identifier) {
                return Err(LocationError::Unsupported {
                    message: format!(
                        "location operator '{identifier}' is not supported in version 1"
                    ),
                    _column: self.column(),
                });
            }
            if has_paren {
                match identifier.as_str() {
                    "complement" => {
                        self.pos += 1;
                        let inner = self.parse_node(depth + 1)?;
                        if matches!(&inner, Node::Complement(_)) {
                            return Err(
                                self.malformed("nested complement() is not a valid location")
                            );
                        }
                        self.skip_whitespace();
                        self.expect(')')?;
                        return Ok(Node::Complement(Box::new(inner)));
                    }
                    "join" => return Ok(Node::Join(self.parse_list(depth + 1)?)),
                    "order" => return Ok(Node::Order(self.parse_list(depth + 1)?)),
                    other => {
                        return Err(self.malformed(&format!("unknown location operator '{other}'")))
                    }
                }
            }
            self.pos = saved;
        }

        self.parse_segment()
    }

    fn parse_list(&mut self, depth: u32) -> Result<Vec<Node>, LocationError> {
        self.expect('(')?;
        self.skip_whitespace();
        if self.peek() == Some(')') {
            return Err(self.malformed("location operator has an empty argument list"));
        }
        let mut nodes = Vec::new();
        loop {
            let node = self.parse_node(depth)?;
            nodes.push(node);
            self.skip_whitespace();
            match self.peek() {
                Some(',') => {
                    self.pos += 1;
                    self.skip_whitespace();
                    if self.peek() == Some(')') {
                        return Err(self.malformed("location operator has a trailing comma"));
                    }
                }
                Some(')') => {
                    self.pos += 1;
                    return Ok(nodes);
                }
                _ => return Err(self.malformed("expected ',' or ')' in a location list")),
            }
        }
    }

    fn parse_segment(&mut self) -> Result<Node, LocationError> {
        self.skip_whitespace();
        let mut start_partial = false;
        let mut end_partial = false;
        let mut prefix = None;
        match self.peek() {
            Some('<') => {
                start_partial = true;
                prefix = Some('<');
                self.pos += 1;
            }
            Some('>') => {
                end_partial = true;
                prefix = Some('>');
                self.pos += 1;
            }
            _ => {}
        }

        self.skip_whitespace();
        let start = self.parse_number()?;
        self.skip_whitespace();

        if self.text[self.pos..].starts_with("..") {
            if prefix == Some('>') {
                return Err(self.malformed("'>' cannot mark the start of a range"));
            }
            self.pos += 2;
            self.skip_whitespace();
            let mut range_end_partial = false;
            match self.peek() {
                Some('>') => {
                    range_end_partial = true;
                    self.pos += 1;
                    self.skip_whitespace();
                }
                Some('<') => return Err(self.malformed("'<' cannot mark the end of a range")),
                _ => {}
            }
            let end = self.parse_number()?;
            if end < start {
                return Err(self.malformed("range end is before its start"));
            }
            return self.segment_node(RawSegment {
                start1: start,
                end1: end,
                start_partial,
                end_partial: range_end_partial,
                zero_len: false,
            });
        }

        if self.peek() == Some('^') {
            if prefix.is_some() {
                return Err(
                    self.malformed("a between-positions site cannot carry a partial marker")
                );
            }
            self.pos += 1;
            self.skip_whitespace();
            let right = self.parse_number()?;
            if right != start + 1 {
                return Err(
                    self.malformed("between positions must be adjacent: n^m requires m = n + 1")
                );
            }
            return self.segment_node(RawSegment {
                start1: start,
                end1: start,
                start_partial: false,
                end_partial: false,
                zero_len: true,
            });
        }

        self.segment_node(RawSegment {
            start1: start,
            end1: start,
            start_partial,
            end_partial,
            zero_len: false,
        })
    }

    fn segment_node(&mut self, segment: RawSegment) -> Result<Node, LocationError> {
        self.segments_seen = self.segments_seen.saturating_add(1);
        if self.segments_seen > self.limits.max_segments_per_feature {
            return Err(LocationError::Fatal(ImportError::LimitExceeded {
                limit: "max_segments_per_feature",
                limit_value: self.limits.max_segments_per_feature,
                actual: self.segments_seen,
                position: Some(self.position()),
            }));
        }
        Ok(Node::Segment(segment))
    }

    fn parse_number(&mut self) -> Result<u64, LocationError> {
        let begin = self.pos;
        let mut value: u64 = 0;
        while let Some(character) = self.peek() {
            let Some(digit) = character.to_digit(10) else {
                break;
            };
            value = value
                .checked_mul(10)
                .and_then(|current| current.checked_add(u64::from(digit)))
                .ok_or_else(|| self.malformed("coordinate is too large"))?;
            self.pos += 1;
        }
        if self.pos == begin {
            return Err(self.malformed("expected a numeric coordinate"));
        }
        if value == 0 {
            return Err(self.malformed("coordinates are 1-based and cannot be zero"));
        }
        if value > self.limits.max_sequence_len {
            return Err(LocationError::Fatal(ImportError::LimitExceeded {
                limit: "max_sequence_len",
                limit_value: self.limits.max_sequence_len,
                actual: value,
                position: Some(self.position()),
            }));
        }
        Ok(value)
    }

    fn try_identifier(&mut self) -> Option<String> {
        let begin = self.pos;
        while let Some(character) = self.peek() {
            if character.is_ascii_alphabetic() || matches!(character, '-' | '_' | '.' | '+') {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.pos == begin {
            return None;
        }
        Some(self.text[begin..self.pos].to_string())
    }

    fn expect(&mut self, expected: char) -> Result<(), LocationError> {
        self.skip_whitespace();
        if self.peek() == Some(expected) {
            self.pos += expected.len_utf8();
            Ok(())
        } else {
            Err(self.malformed(&format!("expected '{expected}'")))
        }
    }

    fn peek(&self) -> Option<char> {
        self.text[self.pos..].chars().next()
    }

    fn skip_whitespace(&mut self) {
        while let Some(character) = self.peek() {
            if character.is_whitespace() {
                self.pos += character.len_utf8();
            } else {
                break;
            }
        }
    }

    fn column(&self) -> u32 {
        let consumed = self.text[..self.pos.min(self.text.len())].chars().count();
        let value = u64::from(self.start_col).saturating_add(consumed as u64);
        u32::try_from(value).unwrap_or(u32::MAX)
    }

    fn position(&self) -> SourcePosition {
        SourcePosition::Genbank {
            line: self.line,
            column: Some(self.column()),
        }
    }

    fn malformed(&self, message: &str) -> LocationError {
        LocationError::Malformed {
            message: message.to_string(),
            column: self.column(),
        }
    }
}

fn is_unsupported_operator(identifier: &str) -> bool {
    matches!(
        identifier.to_ascii_lowercase().as_str(),
        "one-of" | "replace" | "telomere" | "gap"
    )
}

fn flatten(node: Node) -> Result<RawLocation, LocationError> {
    match node {
        Node::Segment(segment) => Ok(RawLocation {
            operator: OPERATOR_SPAN,
            strand: 1,
            segments: vec![segment],
        }),
        Node::Join(children) => flatten_children(children, OPERATOR_JOIN, 1),
        Node::Order(children) => flatten_children(children, OPERATOR_ORDER, 1),
        Node::Complement(inner) => match *inner {
            Node::Segment(segment) => Ok(RawLocation {
                operator: OPERATOR_SPAN,
                strand: -1,
                segments: vec![segment],
            }),
            Node::Join(children) => flatten_children(children, OPERATOR_JOIN, -1),
            Node::Order(children) => flatten_children(children, OPERATOR_ORDER, -1),
            Node::Complement(_) => Err(LocationError::Malformed {
                message: "nested complement() is not a valid location".to_string(),
                column: 1,
            }),
        },
    }
}

fn flatten_children(
    children: Vec<Node>,
    operator: &'static str,
    strand: i8,
) -> Result<RawLocation, LocationError> {
    let mut segments = Vec::new();
    for child in children {
        match child {
            Node::Segment(segment) => segments.push(segment),
            Node::Join(nested) if operator == OPERATOR_JOIN => {
                let flattened = flatten_children(nested, operator, strand)?;
                segments.extend(flattened.segments);
            }
            Node::Order(nested) if operator == OPERATOR_ORDER => {
                let flattened = flatten_children(nested, operator, strand)?;
                segments.extend(flattened.segments);
            }
            _ => {
                return Err(LocationError::Unsupported {
                    message: "mixed location operators or per-segment complement cannot be represented by one operator and strand".to_string(),
                    _column: 1,
                })
            }
        }
    }
    Ok(RawLocation {
        operator,
        strand,
        segments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sequon_model::Topology;

    fn limits() -> Limits {
        Limits::default()
    }

    #[test]
    fn parses_span_and_single_base() {
        let span = parse("123..456", 3, 22, &limits()).expect("span");
        assert_eq!(span.operator, OPERATOR_SPAN);
        assert_eq!(span.strand, 1);
        assert_eq!(span.segments[0].start1, 123);
        assert_eq!(span.segments[0].end1, 456);

        let single = parse("123", 3, 22, &limits()).expect("single");
        assert_eq!(single.segments[0].start1, 123);
        assert_eq!(single.segments[0].end1, 123);
    }

    #[test]
    fn parses_complement_join_order_and_partials() {
        let location = parse("complement(join(<1..>100,200..300))", 3, 22, &limits())
            .expect("complement join");
        assert_eq!(location.operator, OPERATOR_JOIN);
        assert_eq!(location.strand, -1);
        assert_eq!(location.segments.len(), 2);
        assert!(location.segments[0].start_partial);
        assert!(location.segments[0].end_partial);

        let order = parse("order(1..10,20..30,1..10)", 3, 22, &limits()).expect("order");
        assert_eq!(order.operator, OPERATOR_ORDER);
        assert_eq!(order.segments.len(), 3);
    }

    #[test]
    fn parses_between_positions() {
        let location = parse("100^101", 3, 22, &limits()).expect("between");
        assert_eq!(location.segments[0].start1, 100);
        assert!(location.segments[0].zero_len);
    }

    #[test]
    fn rejects_malformed_and_marks_unsupported() {
        assert!(matches!(
            parse("1..(100", 3, 22, &limits()),
            Err(LocationError::Malformed { .. })
        ));
        assert!(matches!(
            parse("one-of(1..2,3..4)", 3, 22, &limits()),
            Err(LocationError::Unsupported { .. })
        ));
        assert!(matches!(
            parse("U49845.1:1..100", 3, 22, &limits()),
            Err(LocationError::Unsupported { .. })
        ));
        assert!(matches!(
            parse("join(complement(1..2),3..4)", 3, 22, &limits()),
            Err(LocationError::Unsupported { .. })
        ));
    }

    #[test]
    fn normalizes_linear() {
        let linear = parse("123..456", 3, 22, &limits()).expect("span");
        let location = normalize(&linear, Topology::Linear, 1000).expect("in range");
        assert_eq!(location.operator, OPERATOR_SPAN);
        assert_eq!(location.segments[0].start, 122);
        assert_eq!(location.segments[0].end, 456);

        let out_of_range = parse("1..1001", 3, 22, &limits()).expect("span");
        assert!(normalize(&out_of_range, Topology::Linear, 1000).is_err());
    }

    #[test]
    fn circular_range_crossing_the_origin_stays_one_unwrapped_segment() {
        let near_origin = parse("5900..6100", 3, 22, &limits()).expect("span");
        let location = normalize(&near_origin, Topology::Circular, 6000).expect("unwrapped span");
        assert_eq!(location.operator, OPERATOR_SPAN);
        assert_eq!(location.segments.len(), 1);
        assert_eq!(location.segments[0].start, 5899);
        assert_eq!(location.segments[0].end, 6100);
        assert!(location.segments[0].is_wrapped(6000));
    }

    #[test]
    fn circular_explicit_join_stays_split() {
        let join = parse("join(5900..6000,1..100)", 3, 22, &limits()).expect("join");
        let location = normalize(&join, Topology::Circular, 6000).expect("split join");
        assert_eq!(location.operator, OPERATOR_JOIN);
        assert_eq!(location.segments.len(), 2);
        assert_eq!(location.segments[0].start, 5899);
        assert_eq!(location.segments[0].end, 6000);
        assert_eq!(location.segments[1].start, 0);
        assert_eq!(location.segments[1].end, 100);
    }

    #[test]
    fn circular_span_longer_than_the_sequence_is_rejected() {
        let over_wrap = parse("1..6001", 3, 22, &limits()).expect("span");
        assert!(normalize(&over_wrap, Topology::Circular, 6000).is_err());
    }
}
