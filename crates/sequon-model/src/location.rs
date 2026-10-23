//! Feature locations: operator, strand and 0-based half-open segments.
//!
//! Segment coordinates are 0-based half-open and are stored exactly as the
//! source syntax expresses them. A linear record keeps every segment inside the
//! sequence (`end <= length`). A circular record may store one unwrapped span
//! whose `end` is greater than `length`; that crossing of the origin is a single
//! segment with at most one wrap, and it resolves on access through
//! [`Segment::resolved`] and [`Location::resolved_segments`]. An explicitly
//! split source (for example a GenBank `join`) stays split, because the segment
//! list records the source syntax; both forms are valid canonical
//! representations of a circular location.

use serde::{Deserialize, Serialize};

/// Operator for a location made of exactly one segment.
pub const OPERATOR_SPAN: &str = "span";
/// Operator for joined segments; segment order is source order.
pub const OPERATOR_JOIN: &str = "join";
/// Operator for ordered segments; the order carries meaning and is never sorted.
pub const OPERATOR_ORDER: &str = "order";

/// A feature location.
///
/// The operator is kept as text so an adapter can carry a parsed token and let
/// Record::validate() reject anything outside the closed set. The strand is a
/// plain i8 for the same reason: the allowed values are -1, 0 and 1, and
/// validation owns the range check rather than the type system, because the
/// canonical JSON contract states the range explicitly.
///
/// Segments are 0-based half-open and stay in source order. On a circular record
/// a single span may cross the origin as one unwrapped segment with `end >
/// length` (at most one wrap); it resolves on access through
/// [`Location::resolved_segments`]. `end > length` never appears on a linear
/// record. A source that explicitly splits a location (for example a GenBank
/// `join` across the origin) keeps its segments split, so both forms are valid
/// canonical representations, each faithful to its source syntax.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Location {
    /// One of span, join or order; validated by Record::validate().
    pub operator: String,
    /// -1 for reverse complement, 0 for unstranded, 1 for forward.
    pub strand: i8,
    /// At least one segment in source order; span carries exactly one. On a
    /// circular record one segment may carry end > length (one wrap).
    pub segments: Vec<Segment>,
}

impl Location {
    /// Builds a span location from one 0-based half-open interval.
    pub fn span(strand: i8, start: u64, end: u64) -> Self {
        Location {
            operator: OPERATOR_SPAN.to_owned(),
            strand,
            segments: vec![Segment::new(start, end)],
        }
    }

    /// Builds a join location from prepared segments.
    pub fn join(strand: i8, segments: Vec<Segment>) -> Self {
        Location {
            operator: OPERATOR_JOIN.to_owned(),
            strand,
            segments,
        }
    }

    /// Builds an order location from prepared segments.
    pub fn order(strand: i8, segments: Vec<Segment>) -> Self {
        Location {
            operator: OPERATOR_ORDER.to_owned(),
            strand,
            segments,
        }
    }

    /// Returns true when any segment crosses the origin of a circular sequence
    /// of `length` bases.
    ///
    /// The topology is not stored on the location, so the caller supplies the
    /// sequence length. A linear record must never contain a wrapped segment;
    /// this helper does not enforce that, it only reports what the coordinates
    /// say.
    pub fn is_wrapped(&self, length: u64) -> bool {
        self.segments
            .iter()
            .any(|segment| segment.is_wrapped(length))
    }

    /// Returns the origin-resolved segments in source order.
    ///
    /// Each segment is resolved independently by [`Segment::resolved`] and the
    /// resulting pieces are concatenated in source order, so a wrapped span
    /// yields its tail-first pair at that segment's position in the list while
    /// every explicitly split segment keeps its own place. Zero-length and
    /// in-range segments resolve to themselves.
    pub fn resolved_segments(&self, length: u64) -> Vec<Segment> {
        self.segments
            .iter()
            .flat_map(|segment| segment.resolved(length))
            .collect()
    }
}

/// One 0-based half-open interval of a location.
///
/// The field order is the canonical JSON field order: start, end,
/// start_partial, end_partial. On a circular record a span that crosses the
/// origin is stored unwrapped, so `end` may exceed the record length (one wrap
/// at most); [`Segment::resolved`] maps it back to origin-ordered pieces. On a
/// linear record every segment satisfies `0 <= start <= end <= length`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Segment {
    /// 0-based inclusive start.
    pub start: u64,
    /// 0-based exclusive end. start == end is a between-positions site. On a
    /// circular record a span segment may exceed the record length by at most
    /// its own start (a single wrap); see [`Segment::resolved`].
    pub end: u64,
    /// True when the source marked the start as partial with a less-than sign.
    pub start_partial: bool,
    /// True when the source marked the end as partial with a greater-than sign.
    pub end_partial: bool,
}

impl Segment {
    /// Builds a segment with both boundaries complete.
    pub fn new(start: u64, end: u64) -> Self {
        Segment {
            start,
            end,
            start_partial: false,
            end_partial: false,
        }
    }

    /// Builds a segment from its four canonical fields.
    pub fn with_boundaries(start: u64, end: u64, start_partial: bool, end_partial: bool) -> Self {
        Segment {
            start,
            end,
            start_partial,
            end_partial,
        }
    }

    /// Returns true when this segment crosses the origin of a circular sequence
    /// of `length` bases.
    ///
    /// A segment is wrapped when its exclusive end is greater than `length`. A
    /// zero-length between-positions site is never wrapped, even when it sits at
    /// the last position of the circular sequence.
    pub fn is_wrapped(&self, length: u64) -> bool {
        self.start < self.end && self.end > length
    }

    /// Returns this segment as one or two origin-resolved segments.
    ///
    /// A zero-length site and an in-range segment resolve to themselves. A
    /// wrapped span resolves to `start..length` followed by `0..(end - length)`,
    /// which is the same bases in the sequence's stored order. Each outer
    /// boundary keeps its partial flag on the piece that owns it: the first piece
    /// keeps `start_partial`, the second keeps `end_partial`, and the boundary
    /// created at the origin is complete. Validation guarantees a wrapped span
    /// has `start < length` and at most one wrap, so the result has two pieces
    /// with `0 <= start < end <= length` each.
    pub fn resolved(&self, length: u64) -> Vec<Segment> {
        if !self.is_wrapped(length) {
            return vec![self.clone()];
        }
        vec![
            Segment::with_boundaries(self.start, length, self.start_partial, false),
            Segment::with_boundaries(0, self.end - length, false, self.end_partial),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_range_and_zero_length_segments_are_not_wrapped() {
        for segment in [
            Segment::new(0, 12),
            Segment::new(3, 8),
            Segment::new(0, 0),
            Segment::new(12, 12),
        ] {
            assert!(
                !segment.is_wrapped(12),
                "{segment:?} must not be reported as wrapped"
            );
            assert_eq!(
                segment.resolved(12),
                vec![segment.clone()],
                "{segment:?} must resolve to itself"
            );
        }
    }

    #[test]
    fn wrapped_span_resolves_into_two_origin_ordered_pieces() {
        let segment = Segment::new(5899, 6100);
        assert!(segment.is_wrapped(6000));
        assert_eq!(
            segment.resolved(6000),
            vec![Segment::new(5899, 6000), Segment::new(0, 100)],
            "a wrapped span resolves to its tail first, then the origin-crossing head"
        );
    }

    #[test]
    fn wrapped_span_keeps_outer_partial_flags_and_completes_the_origin_boundary() {
        let segment = Segment::with_boundaries(5899, 6100, true, true);
        assert_eq!(
            segment.resolved(6000),
            vec![
                Segment::with_boundaries(5899, 6000, true, false),
                Segment::with_boundaries(0, 100, false, true),
            ]
        );
    }

    #[test]
    fn full_circle_span_resolves_into_two_pieces() {
        let segment = Segment::new(5899, 11899);
        assert!(segment.is_wrapped(6000));
        assert_eq!(
            segment.resolved(6000),
            vec![Segment::new(5899, 6000), Segment::new(0, 5899)]
        );
    }

    #[test]
    fn location_wrapping_checks_every_segment() {
        let in_range = Location::join(-1, vec![Segment::new(0, 100), Segment::new(200, 300)]);
        assert!(!in_range.is_wrapped(6000));
        assert_eq!(
            in_range.resolved_segments(6000),
            vec![Segment::new(0, 100), Segment::new(200, 300)]
        );

        let split = Location::join(-1, vec![Segment::new(5900, 6000), Segment::new(0, 100)]);
        assert!(!split.is_wrapped(6000));
        assert_eq!(
            split.resolved_segments(6000),
            vec![Segment::new(5900, 6000), Segment::new(0, 100)],
            "an explicitly split location is already resolved and keeps its order"
        );

        let unwrapped = Location::span(1, 5899, 6100);
        assert!(unwrapped.is_wrapped(6000));
        assert_eq!(
            unwrapped.resolved_segments(6000),
            vec![Segment::new(5899, 6000), Segment::new(0, 100)]
        );
    }

    #[test]
    fn resolved_segments_preserve_source_order_across_segments() {
        let location = Location::join(1, vec![Segment::new(100, 200), Segment::new(5899, 6100)]);
        assert!(location.is_wrapped(6000));
        assert_eq!(
            location.resolved_segments(6000),
            vec![
                Segment::new(100, 200),
                Segment::new(5899, 6000),
                Segment::new(0, 100),
            ]
        );
    }
}
