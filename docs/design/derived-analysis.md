# Derived sequence analysis contract

Status: accepted for R11 implementation, 2026-10-30. <Sequon> owns all
scientific semantics in this document. <NeoAPI-rs> owns the public DTO and
transport envelope described here. No browser, API handler or second service
may redefine these operations.

## 1. Library and public boundaries

The logical <Sequon> library operations are:

- extract a canonical location or selection without changing the Record;
- analyze a Record into CDS translations/codon mappings and restriction sites;
- analyze one selection into requested DNA, reverse-complement, AA and GC
  results;
- return typed errors for invalid, unsupported or over-limit input.

Every input Record first passes Record::validate() and
sequon_algo::verify_digests(). Analysis is pure and deterministic, never writes
results into the Record, and never changes either digest. Algorithm and
database versions are outputs, not optional metadata.

Only neoapi-sequence calls these library operations. Public DTOs are defined
independently in OpenAPI and contain no <Sequon> Rust types. Handlers only
authenticate, check ownership/version/capability, invoke neoapi-sequence, map
DTOs/errors and persist nothing for read-only analysis.

## 1.1 Implemented Sequon API (SEQ-030)

The pure sequon-algo surface now implements this contract without a parser,
HTTP, database or platform identity dependency:

| Function | Result |
| --- | --- |
| extract_location(&Record, &Location) | LocationExtraction with known biological-order segments, per-base mappings and extraction provenance |
| extract_selection(&Record, Selection) | Positive-length topology-checked LocationExtraction |
| gc_content(&str) | GcContent with count,total,percent_milli,ambiguous_count and GC provenance |
| translate_cds(&Record, feature_index) | CdsTranslation with AA/AA3, codon mappings, partial/ambiguity counts and genetic-code provenance |
| translate_location(&Record, &Location, codon_start, transl_table) | Location-level CdsTranslation with the same rules |
| translate_selection(&Record, Selection, SelectionTranslationOptions) | Feature-independent CdsTranslation using explicit strand/frame/table |
| analyze_selection(&Record, Selection, &[SelectionOperation]) | SelectionAnalysis containing only requested DNA/revcomp/GC/translation fields |
| analyze_restriction(&Record, &RestrictionDatabase) | RestrictionAnalysis with confirmed hits, separate ambiguities and DB provenance |
| analyze_record(&Record) | RecordAnalysis using the built-in restriction database |
| analyze_record_with_database(&Record, &RestrictionDatabase) | RecordAnalysis with explicit DB and per-CDS result/error entries |
| restriction_database() / genetic_code_database() | Versioned database definitions/identity including deterministic SHA-256 |

Analysis returns new values and never writes into Record. CdsAnalysis::Error
keeps an unsupported CDS visible by feature_index/code while other CDS results
remain available. The R11 synthetic golden is
[testdata/r11/r11-synthetic-analysis.json](../../testdata/r11/r11-synthetic-analysis.json).

## 2. Coordinate and extraction semantics

### 2.1 Coordinates and UI formatting

Canonical and all public request/response coordinates are 0-based half-open.
For a linear record of length L, a positive-length selection satisfies
0 <= start < end <= L. For a circular record it satisfies 0 <= start < L,
end > start and end <= start + L; end > L denotes at most one origin crossing.
A whole circular molecule may be selected once, never more than once.

UI labels use 1-based closed base positions/ranges: internal [s,e) displays as
s+1..e when no unwrapped circular end is shown. Formatting happens only at the
UI boundary and never changes the stored or wire integers. A zero-length site
is not an empty selection and is not formatted as a base range.

Analysis selection is positive-length. Empty [s,s) input is
SELECTION_EMPTY; it cannot return DNA, GC or translation. Site boundaries and
on-position sites use the explicit rules below instead of empty selections.

### 2.2 Circular resolution

A canonical single circular span remains unwrapped. Resolve it only on access:

- end <= L yields [start,end);
- end > L yields [start,L) followed by [0,end-L).

Extraction concatenates those pieces in that order before applying strand.
start_partial stays on the first resolved piece and end_partial on the last.
Explicitly split join/order locations stay split and are never merged into a
span. A span is not split in the canonical Record. Linear end > L is invalid.

The same rule applies to an unwrapped selection, codon-mapping segment or
recognition hit. Circular cut boundaries are normalized into 0..L, where 0 and
L denote the same origin boundary but are not two different cuts.

### 2.3 span, join, order and strand

- span has exactly one segment. Its extracted sequence is that segment's
  resolved bases.
- join concatenates segments in canonical source order, resolving each circular
  segment independently. No filler bases are inserted.
- order also returns the known segments in canonical source order, but its
  continuity value is ordered_unknown_gaps. No claim is made that the
  concatenation is a complete biological molecule or CDS. order is never
  sorted.
- strand 1 leaves the concatenated sequence unchanged. strand -1 computes the
  reverse complement of the complete concatenation once; this reverses the
  biological segment order and complements every base. It does not rewrite the
  canonical segments. This is whole-location complement semantics; mixed
  per-segment orientations are not representable and remain unsupported.
- strand 0 permits recorded-order DNA extraction only. Feature-oriented
  reverse-complement or CDS translation is STRAND_UNSUPPORTED.

Reverse complement uses the canonical IUPAC map A<->T, C<->G, R<->Y, S<->S,
W<->W, K<->M, B<->V, D<->H and N<->N, then reverses the string.

### 2.4 Zero-length and between-position sites

A canonical zero-length segment [s,s) is a between-position site at boundary
s, not an empty feature. It never wraps and yields no DNA. It may be rendered
and indexed with the platform site_kind discriminator, but it is rejected as an
analysis selection.

An on-position site is separately discriminated and maps to the one-base
selection [s,s+1) for UI navigation. Restriction cuts are between-base
boundaries and retain that identity even when the UI also exposes a one-base
anchor for navigation.

## 3. Record-level analysis bundle

### 3.1 Public routes

Contract only; implementation is deferred to SEQ-060:

- GET /sequences/{record_id}/analysis returns the bundle for the resolved
  current version;
- GET /sequences/{record_id}/versions/{version}/analysis returns the bundle for
  exactly that version.

Both are owner-only and return 404 not_found for a missing, foreign or unknown
version. The current route resolves once and thereafter behaves exactly like
the versioned route. The bundle route requires qualified translation and
restriction capabilities; it never silently omits an unavailable component.

### 3.2 SequenceAnalysisBundle DTO

The envelope schema is neoapi.sequence-analysis/1 and contains:

| Field | Content |
| --- | --- |
| identity | record_id, version, payload_sha256, schema_urn, schema_sha256 and digest_algorithm, exactly matching the source document |
| record | length and topology |
| provenance | producer name/version, <Sequon> revision, algorithm ids/versions and database ids/versions/SHA-256 values |
| cds | One CdsTranslation or CdsAnalysisError for every canonical CDS, in feature source order |
| restriction_sites | Confirmed RestrictionHit objects in deterministic order |
| restriction_ambiguities | Possible but unconfirmed pattern matches, separately classified and never counted as cuts |

A CdsAnalysisError contains feature_index and a typed code/details. One
unsupported CDS does not hide other CDS results or get dropped from the
response. Invalid Record identity/digests or a resource-limit failure rejects
the whole bundle; there is no truncated partial bundle.

### 3.3 CDS selection and independence

Only features whose key is exactly CDS receive CDS translation. Their stored
/translation qualifier is preserved as source metadata but is never used as the
computed answer. Every CDS is analyzed independently in canonical feature
source order.

Overlapping CDS features are not merged, sorted by coordinate, deduplicated by
name or assigned a shared frame. Each uses its own location, strand,
codon_start and transl_table. feature_index is the version-scoped identity.
A selection spanning more than one CDS has no single-CDS identity; record-level
tracks still retain all independent mappings.

### 3.4 codon_start and reverse-strand frames

For CDS, zero or one codon_start qualifier is allowed. Absence means 1; exactly
one value must parse as 1, 2 or 3. More than one occurrence, an invalid integer
or a value outside that set is CODON_START_INVALID. The algorithm skips
codon_start - 1 bases from the 5' end of the strand-oriented, extracted CDS.

Thus a reverse-strand CDS is completely extracted and reverse-complemented
before its offset is applied. Its frame is never computed by reusing the
forward-strand frame or by reversing only each segment independently. The
offset must not exceed the extracted length; otherwise the result is
CODON_START_INVALID. An offset equal to that length yields an empty translation
with zero codons. start_partial/end_partial markers remain metadata and do not
silently add, remove or pad bases.

### 3.5 transl_table

Zero or one transl_table qualifier is allowed. Absence means NCBI table 1;
more than one occurrence is TRANSL_TABLE_INVALID. R11 supports tables 1 and 11
from the pinned NCBI genetic-code database. Other integer values and non-integer
values return TRANSL_TABLE_UNSUPPORTED for that CDS.

Tables 1 and 11 have the same elongation mapping in the pinned database. The
operation translates every complete codon; it does not predict initiation or
force an alternative start codon to methionine. ATG translates as M/Met, and a
stop translates as */Ter according to the selected table. Algorithm/database
provenance includes the exact table bytes/version.

### 3.6 Ambiguous codons and partial tails

Each IUPAC codon is expanded to its concrete ACGT possibilities. If every
possibility maps to the same amino acid, that amino acid is emitted; for
example GCN yields A/Ala. If all possibilities are stops, */Ter is emitted.
Otherwise X/Xaa is emitted. A mixed stop/amino-acid possibility is X/Xaa and
increments ambiguous_stop_codon_count. No ambiguous codon is guessed from its
most likely base.

After the codon_start offset, complete 3-base codons are translated. A trailing
one- or two-base tail produces no amino acid and no padding. The result reports
partial_tail_length (0, 1 or 2), partial_tail_dna and the original partial
boundary flags. Non-triplet CDS length is therefore explicit data, not an
implicit error and never a hidden truncation.

CDS using operator order returns ORDER_CDS_UNSUPPORTED instead of translating
through unknown gaps. join is translated by the rules above. A CDS with strand
0 returns STRAND_UNSUPPORTED.

### 3.7 CdsTranslation DTO and codon mapping

A successful entry contains:

- feature_index, strand, codon_start, transl_table and continuity;
- source_length (extracted bases before the offset), translated_length (bases
  consumed by complete codons, exactly three times translated_codon_count) and
  translated_codon_count (exactly the aa/aa3/codons length);
- aa as the one-letter string and aa3 as the aligned array of three-letter
  symbols (Xaa for unknown, Ter for stop);
- ambiguous_codon_count, ambiguous_stop_codon_count, stop_codon_count;
- partial_tail_length, partial_tail_dna, start_partial and end_partial;
- codons in biological translation order.

Each codon entry contains aa_index, codon, aa, aa3 and segments. segments is a
non-empty array of {start,end} coordinate pieces in biological 5'->3' order,
covering exactly three bases. A codon may cross the origin or a join boundary,
so one coarse interval is insufficient. On the reverse strand, biological
codon order runs toward decreasing recorded coordinates while each individual
[start,end) piece remains half-open. The UI uses this server mapping to align a
single AA click with exactly three bases; it does not reconstruct translation
or reverse-strand frames itself.

## 4. Selection-level DNA, reverse complement, AA and GC

### 4.1 Route and request

Contract only: POST /sequences/{record_id}/versions/{version}/analysis/selection.
The version is explicit so a current-version race cannot attach a result to the
wrong document. The owner-only SequenceSelectionAnalysisRequest has:

| Field | Rule |
| --- | --- |
| selection | {start,end} using the positive-length linear/circular rules in section 2 |
| operations | Non-empty unique array containing dna, revcomp, gc and/or translation |
| translation | Required when translation is requested and forbidden otherwise: {strand: 1 or -1, frame: 0, 1 or 2, transl_table: 1 or 11} |

Selection translation does not inherit a CDS, name or overlapping-feature
frame. frame is the number of bases to skip from the selected sequence's 5'
end. For strand -1, reverse-complement the entire selection first and then skip
frame bases. frame greater than the selected length is sequence_selection_invalid;
frame equal to it yields zero codons. A feature-track selection uses the
record-level CDS codon mapping instead of guessing a selection frame.

### 4.2 SequenceSelectionAnalysis DTO

The envelope schema is neoapi.sequence-selection-analysis/1. It repeats the
complete document identity, selection, operations and provenance, then includes
only requested result fields:

- dna: the exact selected bases in recorded forward-strand order;
- revcomp: their IUPAC reverse complement;
- translation: the same CdsTranslation scalar/array/codon fields applicable to
  a selection, using the explicit strand/frame/table;
- gc: {count,total,percent_milli,ambiguous_count}.

GC count includes G, C and S because each of those symbols guarantees G or C.
All other IUPAC ambiguity symbols contribute zero to count; they are not
converted to probabilities. total is every selected base, including ambiguity
symbols. ambiguous_count is the number of non-ACGT symbols, including S.
percent_milli is percent multiplied by 1000 and rounded to the nearest integer
with exact half-up rational arithmetic: floor((count * 100000 * 2 + total) /
(2 * total)). Thus 100 percent is 100000. The UI formats this integer but does
not implement a separate GC rule.

No selection result is cached under a shorter key or returned for a different
version/selection/parameter set after an aborted request.

## 5. Restriction-site semantics

### 5.1 Database and matching

The restriction database is bundled and versioned by <Sequon>. Every entry has
a stable enzyme_id, 5'->3' IUPAC recognition pattern, top/bottom cut offsets and
signed overhang description. The response records database id, version and
SHA-256. No browser list or handler table is a second database.

Pattern matching compares IUPAC possibility sets in 5'->3' orientation. Both
the recorded strand and its reverse complement are examined. A confirmed hit
requires the candidate base possibility set to be a subset of the pattern set at
every position; this includes guaranteed degenerate matches. A candidate that
only intersects a pattern is emitted in restriction_ambiguities with
AMBIGUOUS_TEMPLATE, never as a confirmed cut. Unsupported pattern/base symbols
produce a typed database or input error rather than a wildcard guess.

A linear match must fit within 0..L. A circular match may cross the origin once
and uses an unwrapped [start,end) recognition range. Palindromic recognition
sequences produce one biological hit, not duplicate forward/reverse hits.
A palindromic hit has recognition strand 0 and carries its explicit top/bottom
cuts. Non-palindromic orientations remain distinct. Confirmed hits sort by
(start,end,enzyme_id,strand); ambiguous candidates use the same order.

### 5.2 Cut coordinates and navigation anchors

Each RestrictionHit contains enzyme_id, recognition {start,end,strand}, cuts,
overhang and anchor. Each cut contains strand and boundary, where boundary is
the between-base position after applying the database offset in that strand's
5'->3' direction. For recognition start s, pattern length p and a database
offset o, a forward cut boundary is s+o and a reverse cut boundary is
s+(p-o), resolved in the unwrapped coordinate system before normalization.
Reverse offsets are never reused as forward offsets. Circular boundaries
normalize to 0..L. Cut boundaries are not zero-length DNA selections.

For R11 navigation, anchor is a separate one-base {start,end,side} selection.
It selects the base immediately to the right of the primary cut when one
exists. A circular origin cut uses [0,1). At linear boundary L there is no
right base, so it uses [L-1,L) with side=left. This display anchor never
changes the cut boundary or recognition range.

## 6. Provenance and cache identity

Every bundle/selection response carries:

- producer: neoapi-sequence crate name/version and the approved <Sequon>
  revision;
- algorithms: stable ids and independent versions for extraction,
  reverse-complement, GC, translation and restriction analysis;
- databases: NCBI genetic-code and <Sequon> restriction database ids, versions
  and SHA-256 values;
- limit_profile: r11-analysis-v1.

The complete cache key is:

(record_id, version, payload_sha256, operation/bundle kind, selection,
ordered operations, translation strand/frame/table, algorithm ids/versions,
database ids/versions/hashes, producer revision, limit_profile,
schema_urn/schema_sha256, digest_algorithm).

For a record bundle, selection and selection translation fields are absent from
the key. For selection analysis, every field is explicit. A cache hit must
revalidate ownership and capability. Error, aborted, stale-response or
capability-unknown results are not cached as successful science.

## 7. Resource contract: r11-analysis-v1

| Limit | Value | Failure |
| --- | --- | --- |
| Record sequence length | 2,000,000 bp | 413 sequence_analysis_limit_exceeded |
| Selection length | 1,000,000 bp | 413 sequence_analysis_limit_exceeded |
| CDS features per record | 5,000 | 413 sequence_analysis_limit_exceeded |
| Total canonical segments | 100,000 | 413 sequence_analysis_limit_exceeded |
| Restriction patterns | 256 | 413 sequence_analysis_limit_exceeded |
| Confirmed + ambiguous restriction hits | 100,000 | 413 sequence_analysis_limit_exceeded |
| Bundle response JSON | 32 MiB | 413 sequence_analysis_limit_exceeded |
| Selection response JSON | 16 MiB | 413 sequence_analysis_limit_exceeded |
| Concurrent blocking analyses per process | 2 | 429 sequence_analysis_concurrency_limited when the bounded queue is full |
| Queued analyses per process | 8 | 429 sequence_analysis_concurrency_limited when full |
| Concurrent analyses per owner | 1 | 429 sequence_analysis_concurrency_limited when full |
| Record-bundle deadline | 10 seconds | 504 sequence_analysis_deadline_exceeded |
| Selection-analysis deadline | 5 seconds | 504 sequence_analysis_deadline_exceeded |

CPU work runs in bounded blocking execution, never directly on the async
executor. Input and estimated output are checked before allocation where
possible. Exceeding any limit fails the whole operation without truncating a
sequence, translation, hit list or JSON response. Error details may name the
limit and configured/observed integers, never sequence content.

A deployment may use a lower limit profile for safety, but the profile name and
values must be explicit and included in cache/provenance. Raising a value is a
new reviewed profile, not a silent configuration drift.

## 8. Typed errors and capability gates

Library errors are typed and independent of HTTP. Public mapping uses the
existing ErrorEnvelope and these stable codes:

| Condition | HTTP/public code |
| --- | --- |
| Empty or out-of-range selection | 422 sequence_selection_invalid |
| Unsupported location/strand/order CDS | 422 sequence_location_unsupported |
| Invalid codon_start | 422 sequence_codon_start_invalid |
| Invalid or unsupported transl_table | 422 sequence_translation_table_unsupported |
| Input/output limit exceeded | 413 sequence_analysis_limit_exceeded |
| Bounded queue/concurrency full | 429 sequence_analysis_concurrency_limited |
| Deadline exceeded | 504 sequence_analysis_deadline_exceeded |
| Corrupt stored Record or digest | 500 sequence_payload_corrupt |
| Incompatible stored Record | 503 sequence_payload_incompatible |
| Missing/foreign record/version | 404 not_found |
| Operation absent | 501 not_implemented |
| Capability state unknown | 503 capability_unknown |
| Capability stale, missing or unqualified | 503 capability_unavailable |

Capabilities sequence.analysis.translation, sequence.analysis.restriction and
sequence.analysis.selection are independent. The bundle route requires the
first two; selection translation additionally requires sequence.analysis.selection.
Unknown never means available, flags/fakes never establish qualification, and
no partial result is presented as an available capability. Document reading is
unaffected by analysis capability failure.

## 9. Fixture and golden-vector separation

- R11 synthetic fixtures are explicitly named and marked synthetic. The 6,142 bp
  R11 construct, ATG/CAT/M/Met, reverse-strand KanR A/Ala and 812-AA examples
  test the agreed display/analysis contract only. They are not GenBank or
  SnapGene parser evidence and are not represented as biological reference
  records.
- Real format fixtures remain in the existing <Sequon> GenBank/SnapGene corpora
  and the corresponding NeoAPI fixture directories. Import conformance uses
  those real fixtures, including join, order, complement, near-origin and
  partial cases; synthetic R11 data never substitutes for them.
- Analysis goldens are reviewed against this contract and pinned algorithm /
  database versions. They are not generated by browser code or a second
  scientific implementation.
- Unknown molecule kinds, unsupported location forms/tables, invalid
  coordinates and ambiguous-only possible restriction hits are surfaced with
  typed errors or explicit ambiguity entries. No unsupported input is silently
  guessed, dropped, hidden or advertised as a confirmed scientific result.
