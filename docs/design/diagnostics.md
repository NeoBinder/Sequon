# Diagnostics

**Status: accepted, M0 contract.** This document is the source of truth for the
diagnostic code vocabulary carried by `sequon/import-report/1` and by typed
import and write errors. It fixes the code table version `2026-10-21/1`, the
severity of every code, the source position each code can carry, and the exact
ordering rule. The machine-readable shape of a diagnostic and of a source
position is `schemas/import-report-1.schema.json`; byte-level JSON rules are in
[canonical-json-profile.md](canonical-json-profile.md).

## 1. Scope and ownership

- Sequon owns this vocabulary. Adapters emit codes from this table and never
  invent one inline.
- A code is a stable identifier for a class of event. Consumers switch on the
  code and on the severity in the report; they must not parse `message` text.
- Codes shared by more than one adapter are owned by `core` (the module that
  defines the shared vocabulary); format-specific codes are owned by the adapter
  named in the prefix (`GB_` for the GenBank adapter, `DNA_` for the SnapGene
  adapter, `WRITE_` for the GenBank writer).
- Severity is fixed per code. A code never changes severity.

## 2. Where diagnostics appear

- Every warning and informational event goes to `diagnostics[]` of the import
  report. It never goes into `provenance`, and it never becomes part of a
  `Record` (D6/D7). A record version is immutable: editing a record produces a
  new version that does not inherit the old import diagnostics.
- Error-severity codes are carried by the typed `ImportError` variants
  (`Malformed`, `Unsupported`, `MissingRequired`, `LimitExceeded`) identified by
  their stable code string. An import that aborts produces no report.
- A report may contain an error-severity diagnostic only when the import still
  returns at least one usable record, for example a multi-record GenBank file in
  which one record failed. Such an entry always carries the `record_index` of
  the failed record, and the remaining records stay valid.
- A failure that leaves no usable record returns a typed `ImportError` and
  produces no report at all. In that case the code travels in the error, never in
  `diagnostics[]`.
- Writer codes are separate from the import vocabulary: error-severity `WRITE_*`
  codes are returned on `WriteError`, and warning or info `WRITE_*` codes are
  returned in `WriteOutcome.diagnostics`. None of them appears in an import
  report.
- `record_index` is the 0-based parse-order ordinal of the record, not
  necessarily an offset into `records[]`. Under `multiple = First` the report
  still describes every parsed record, so a diagnostic for a non-returned record
  can carry an index that has no `records[]` entry.
- `dropped[]` and `preserved[]` are not diagnostics; they are the auditable
  record of blocks or features that were not mapped. They are described in
  section 8.

## 3. `codes_version`

- The import report requires `codes_version`; it identifies the code table that
  produced the report. A report without it is invalid.
- Current value: `2026-10-21/1`. The format is a date, a slash, and a positive
  decimal revision counter: `YYYY-MM-DD/N`. The date is the date the table
  changed; `N` distinguishes revisions made on the same date. A second change on
  the same day becomes `2026-10-21/2`; the next distinct day starts at `/1`.
- The table is **append-only**. Adding a code bumps `codes_version`. An existing
  code keeps its spelling, severity, position kind and meaning forever. Changing
  the meaning of an existing code is forbidden: add a new code instead, and
  record the old one as deprecated without reusing its spelling.
- `2026-10-21/1` is the initial freeze. It was extended during pre-release M0/M2
  with the lead-approved additions (nine codes and two reasons) before any
  consumer pinned it, so it remains `/1`; after the first release, every change
  bumps the revision.
- Removing a code requires deprecating it here first and still bumps
  `codes_version`; a removed code is never reused for a different event.
- `codes_version` is independent of the schema version and of the crate version:
  a parser release that adds a code bumps `codes_version`, and a schema release
  that adds a field does not.

## 4. Consumer behavior for unknown codes

- A report may contain a code that a consumer does not know, because the producer
  is newer than the consumer.
- The consumer MUST keep the raw code string, keep the severity, `message` and
  position unchanged, and MUST NOT reject the report because of an unknown code.
- An unknown code is mapped to a generic unknown class for filtering or display
  purposes; the raw string is still what gets stored and quoted back.
- A consumer MUST NOT drop unknown diagnostics silently. If it filters to a known
  set, it surfaces unknown ones at least once (an aggregate log line or a count
  is enough).
- Severity in the report is authoritative. A consumer never derives severity from
  a code-name pattern, and never upgrades warning to error.

## 5. Source positions

A diagnostic or a `dropped[]` entry may carry a source position. Two shapes
exist, discriminated by `kind`:

```json
{ "kind": "genbank", "line": 120, "column": 22 }
{ "kind": "snapgene", "offset": 40960, "packet_type": 7, "packet_index": 12 }
```

GenBank positions:

- `line` is required and is 1-based. Lines are counted on the CRLF-normalized
  text, so `line` is the same number a text editor shows after newline
  normalization; the first line of the file is line 1.
- `column` is 1-based and optional. It is present only when the adapter knows the
  column (for example a malformed location or qualifier); line-only codes omit
  it instead of inventing a column.

SnapGene positions:

- `offset` is required and is the 0-based byte offset from the start of the file.
- `packet_type` and `packet_index` are optional and are present only when the
  position belongs to a TLV packet. Cookie-level or header-level errors carry
  `offset` alone.
- `packet_index` is 0-based and counts TLV packets in file order, with the cookie
  packet as index 0.

Common rules:

- A report describes one source format, so every position in it has the same
  `kind`; a report never mixes GenBank and SnapGene positions.
- Positions refer to the source, never to canonical coordinates. Canonical
  coordinates are 0-based half-open and live only in `Record`.
- A position is a location hint for a human, not a stable identity. Byte offsets
  in a re-exported file change; the diagnostic code and the record index do not.
- The JSON Schema constrains `line >= 1`, `offset >= 0`, `column >= 1` when
  present, and non-negative packet fields. The applicability of a field per code
  is the table below, not the schema.
- A `record_index` identifies a record by parse order (section 2); it is not
  necessarily a valid index into `records[]`.

## 6. Ordering

`diagnostics[]` is sorted with a stable sort over a deterministic emission order.
The sort key is a tuple, compared lexicographically:

1. `position_rank`: `0` when the entry has a position, `1` when it has none, so
   positioned events come first in ascending source order and unpositioned events
   come last.
2. `primary`: GenBank `line`, or SnapGene `offset`.
3. `secondary`: GenBank `column` (0 when absent), or SnapGene `packet_index` (0
   when absent).
4. `tertiary`: 0 for GenBank, SnapGene `packet_type` (0 when absent).
5. `code`, compared as ASCII bytes in ascending order.
6. Emission index: the position in which the adapter produced the entry, used as
   the stable tie-break so the result is total.

Two imports of the same bytes must produce the same order. An unstable sort, or
a sort that depends on a hash map iteration order, is a defect.

## 7. Messages

- `message` is a short English sentence explaining the event for a human. It is
  not machine-readable and its wording is not part of the contract; see
  section 10 of [canonical-json-profile.md](canonical-json-profile.md) for what
  that leaves unpromised.
- A message never contains user sequence content, raw source bytes, a qualifier
  value, or any substring copied from the input payload. It may name a field, a
  feature key, a limit name, a count, a length, or a position.
- A message is one line: no newline, no tab, no carriage return, no control
  character.
- A message never contains a machine path, a host name, a user name, an
  environment value, or a credential.
- Rationale: reports are logged and shared; sequence content is the user data and
  must not leak through diagnostics (see `Sequon/AGENTS.md`).

## 8. `dropped[]` vocabulary

An entry that could not be represented in the canonical model is either kept as
raw bytes in `preserved[]` or fingerprinted in `dropped[]`; the two are mutually
exclusive for the same range. Bytes that are kept go to `preserved`; bytes that
are only described go to `dropped`.

`what` is a snake_case kind defined by the adapter. v1 defines these values:

| `what` | Meaning |
| --- | --- |
| `feature` | A feature skipped under D11. `feature_key` and `location_text` are required for this kind. |
| `trace_container` | A SnapGene trace container packet kept only as a fingerprint. |
| `unknown_packet` | A SnapGene packet whose type the reader does not know and which could not be preserved. |
| `alignable_sequence` | A SnapGene alignable-sequence block kept only as a fingerprint. |
| `history_tree` | A SnapGene history or history-modifier block kept only as a fingerprint. |

`reason` is a closed, append-only vocabulary. It is the machine-readable reason
for the loss. When a diagnostic code explains the same loss, that code is
mentioned in the paired diagnostic, and the reason stays one of these values:

| `reason` | Used when | Paired diagnostic |
| --- | --- | --- |
| `size_limit` | The block exceeded the preserved-byte budget, so only offset, length and digest are kept. | none (the `dropped[]` entry is the record) |
| `unsupported_location` | A feature location cannot be represented in v1 (for example a remote or `one-of` location). | `GB_LOCATION_UNSUPPORTED` (warning) |
| `coordinate_out_of_range` | A feature coordinate falls outside the sequence. | `GB_COORDINATE_OUT_OF_RANGE` (GenBank) or `DNA_FEATURE_RANGE_INVALID` (SnapGene), both warnings |
| `malformed_metadata` | A feature XML entry lacks usable metadata (no key/type or no segment), so it cannot be mapped. | `DNA_FEATURE_MALFORMED` (warning) |
| `not_requested` | The caller set `preserve_unhandled_blocks = false`, so the block was fingerprinted instead of kept. | The block's diagnostic (`DNA_PACKET_IGNORED` or `DNA_UNKNOWN_PACKET`), if any |

Adding a reason follows the same append-only rule as a code: add the row here, do
not repurpose an existing value, and bump `codes_version` when the table changes.
The JSON Schema cannot express this dependency; adapter tests enforce it.

## 9. Code table

Table version `2026-10-21/1`. Counts: 26 GenBank, 28 SnapGene, 5 shared, 6 writer;
65 codes total. `LIMIT_EXCEEDED` is the code of the typed `LimitExceeded` variant
rather than of a report diagnostic (see the notes).

Position values: `none`, `line`, `line + column`, `offset`, `offset + packet`,
`line or offset`. `offset + packet` means the position names the packet payload
offset and also carries `packet_type` and `packet_index`; both packet fields are
optional in the schema, so a producer that knows only the offset still produces a
valid position. The plan table groups every SnapGene position as `offset`, and
this table records the precise case; `line + column` is the plan's `line,col`.

### 9.1 GenBank (`genbank`)

| Code | Severity | Position | Meaning |
| --- | --- | --- | --- |
| `GB_EMPTY_INPUT` | error | none | The input is empty: no bytes, or only whitespace. |
| `GB_CONTENT_BEFORE_LOCUS` | error | line | Non-empty content appears before the first `LOCUS` line; the file is not a GenBank record. |
| `GB_MISSING_LOCUS` | error | line | No `LOCUS` line was found where a record should start. |
| `GB_MALFORMED_LOCUS` | error | line | The `LOCUS` line could not be parsed into name, length, molecule type and topology. |
| `GB_RECORD_UNTERMINATED` | error | line | The record has no terminating `//` before end of input. |
| `GB_MISSING_ORIGIN` | error | none | The record has no `ORIGIN` section. |
| `GB_SEQUENCE_EMPTY` | error | none | The `ORIGIN` section yields zero bases. |
| `GB_ORIGIN_INVALID_CHAR` | error | line | The `ORIGIN` section contains a character outside the IUPAC nucleotide set. |
| `GB_RNA_BASE_IN_DNA` | error | line | A DNA record contains `U`; RNA input is not silently converted. |
| `GB_UNSUPPORTED_MOLECULE` | error | line | The `LOCUS` molecule type is RNA or protein; v1 implements DNA only. |
| `GB_TOPOLOGY_MISSING` | error | line | The `LOCUS` line states no topology and no topology hint was supplied. |
| `GB_LOCATION_MALFORMED` | error | line + column | A feature location cannot be parsed: unbalanced parentheses or non-numeric bounds. |
| `GB_MULTIPLE_RECORDS_NOT_ALLOWED` | error | line | The file holds more than one record while the policy is `Reject`. |
| `GB_LOCUS_LENGTH_MISMATCH` | warning | line | The `LOCUS` declared length differs from the actual `ORIGIN` length; the actual length wins. |
| `GB_LOCATION_UNSUPPORTED` | warning | line | The feature location uses a construct v1 cannot represent; the feature is skipped and recorded in `dropped[]`. |
| `GB_COORDINATE_OUT_OF_RANGE` | warning | line | A feature coordinate exceeds the sequence of a linear record; the feature is skipped and recorded in `dropped[]`. |
| `GB_UNKNOWN_FEATURE_KEY` | info | line | The feature key is not in the known set; it is kept verbatim. |
| `GB_QUALIFIER_UNQUOTED` | info | line | A qualifier value is unquoted (a bare value); it is kept as written. |
| `GB_LOCATION_BETWEEN_POSITIONS` | info | line | A between-positions site `n^m` was mapped to a zero-length segment with `start == end`. |
| `GB_LOCATION_PARTIAL_BOUNDARY` | info | line | A `<` or `>` boundary marker was preserved as `start_partial` or `end_partial`. |
| `GB_FEATURE_COLUMN_MISALIGNED` | info | line | The `FEATURES` columns drifted from the standard offsets; the line was parsed tolerantly. Emitted at most once per file. |
| `GB_SEQUENCE_NORMALIZED` | info | none | Lowercase bases in `ORIGIN` were uppercased. `ORIGIN` line numbers, grouping spaces and line breaks are structural formatting, not a normalization event. |
| `GB_CRLF_NORMALIZED` | info | none | CRLF line endings were normalized to LF before parsing. |
| `GB_HEADER_FIELD_UNKNOWN` | info | line | A header key outside the recognized set was preserved in `extras.genbank.header`. |
| `GB_ENCODING_INVALID` | error | line | The source is not valid UTF-8 where text is required (header value, qualifier, or sequence text); v1 rejects instead of decoding lossily. |
| `GB_FEATURES_MALFORMED` | error | line | The FEATURES block is structurally broken independently of a location: stray qualifier or continuation, empty feature key, or a bare qualifier followed by a continuation line. |

### 9.2 SnapGene (`.dna`, `snapgene`)

| Code | Severity | Position | Meaning |
| --- | --- | --- | --- |
| `DNA_BAD_MAGIC` | error | offset | The file does not start with the SnapGene cookie packet; it is not a `.dna` container. |
| `DNA_TRUNCATED_HEADER` | error | offset | The cookie or container header is incomplete. |
| `DNA_TRUNCATED_PACKET` | error | offset + packet | A packet length reaches past the end of the file. |
| `DNA_PACKET_LENGTH_INVALID` | error | offset + packet | A packet length field is invalid, for example a length that overflows or cannot be allocated within limits. |
| `DNA_NO_DNA_PACKET` | error | none | The container contains neither an uncompressed nor a compressed DNA packet. |
| `DNA_DUPLICATE_DNA_PACKET` | error | offset | Any second DNA packet is present (type 0 or 1, same or different type); no implicit choice is made. |
| `DNA_COMPRESSED_LENGTH_INVALID` | error | offset | The 2-bit DNA packet is not self-consistent with its declared length. |
| `DNA_DECOMPRESSION_LIMIT` | error | offset | LZMA output exceeded the configured size or ratio limit; the stream is stopped and the import fails. |
| `DNA_LZMA_MALFORMED` | error | offset | The LZMA stream is corrupt. |
| `DNA_XML_MALFORMED` | error | offset + packet | An XML block is not well-formed. |
| `DNA_XML_DEPTH_EXCEEDED` | error | offset + packet | XML nesting exceeded `max_xml_depth`; the block is rejected instead of expanded. |
| `DNA_SEQUENCE_INVALID_CHAR` | error | offset | A DNA packet contains a base outside the accepted character set. |
| `DNA_EXPORT_VERSION_UNSUPPORTED` | error | offset | The export-version field cannot be parsed. |
| `DNA_UNKNOWN_SEQ_TYPE` | error | offset | The cookie sequence type is outside the known enumeration. |
| `DNA_UNSUPPORTED_MOLECULE` | error | offset | The container holds a non-DNA sequence type; v1 reads DNA only. |
| `DNA_TOPOLOGY_MISSING` | error | offset | The DNA flags cannot provide a topology and no topology hint was supplied. |
| `DNA_UNKNOWN_PACKET` | info | offset + packet | A packet of unknown type was preserved in `preserved[]` under the preserved-byte budget. |
| `DNA_PACKET_IGNORED` | info | offset + packet | A known but unmapped packet (for example trace, alignable-sequence or history-modifier content) was preserved or fingerprinted. |
| `DNA_FEATURE_RANGE_INVALID` | warning | offset + packet | A feature coordinate falls outside the sequence; the feature is skipped and recorded in `dropped[]`. |
| `DNA_HISTORY_PRESERVED_RAW` | info | offset + packet | The history tree was kept as raw bytes and is not mapped to a family tree. |
| `DNA_HISTORY_DECODE_FAILED` | warning | offset + packet | The history block could not be decoded; its raw bytes are kept and the record is still usable. |
| `DNA_TOPOLOGY_FLAG_UNKNOWN` | info | offset | An unknown DNA-flags bit is set; it is recorded and its meaning is not guessed. |
| `DNA_TRAILING_BYTES` | warning | offset | Non-zero trailing bytes follow the last packet; they are ignored. |
| `DNA_SEQUENCE_EMPTY` | error | offset | A DNA packet decodes to zero bases; a zero-length sequence is not a record. |
| `DNA_SEQUENCE_NORMALIZED` | info | offset | The uncompressed DNA packet contained lowercase letters or whitespace, normalized to uppercase IUPAC with no loss. |
| `DNA_FEATURE_MALFORMED` | warning | offset + packet | A feature XML entry lacks usable metadata (key/type or segment); the feature is skipped and recorded in `dropped[]`. |
| `DNA_HISTORY_DECODE_FALLBACK` | info | offset + packet | The version-selected history decode failed and the documented fallback path succeeded; the version rule remains primary. |
| `DNA_TOPOLOGY_CONFLICT` | warning | offset | A notes/properties value disagrees with the DNA flags topology; the flags win and no value is rewritten. |

### 9.3 Shared (`core`)

| Code | Severity | Position | Meaning |
| --- | --- | --- | --- |
| `RECORD_REQUIRED_FIELD_MISSING` | error | line or offset | A required record field (`name`, `kind` or `topology`) has no source and no hint; the message and the typed error name the field. |
| `RECORD_FIELD_INVALID` | error | line or offset | A record field is present but its value is invalid, for example an unknown topology. |
| `MOLECULE_TYPE_UNKNOWN` | warning | line or offset | A molecule subtype outside the known set was mapped to `other`. |
| `EXTRAS_NAMESPACE_COLLISION` | warning | none | Two adapters attempted to write the same `extras` namespace; the first writer keeps the namespace. |
| `LIMIT_EXCEEDED` | error | none | A configured limit was exceeded. This is the code of the typed `LimitExceeded` variant, not a report diagnostic. |

### 9.4 Writer (`writer`)

Error-severity codes below are carried by `WriteError`; warning and info codes
are carried in `WriteOutcome.diagnostics`. None of them appears in an import
report.

| Code | Severity | Position | Meaning |
| --- | --- | --- | --- |
| `WRITE_UNSUPPORTED_KIND` | error | none | The record is not `kind: dna`; v1 writes DNA GenBank only. |
| `WRITE_NAME_TRUNCATED` | warning | none | The record name exceeded the `LOCUS` name field and was truncated. |
| `WRITE_QUOTE_NORMALIZED` | info | none | A qualifier value contained a quote; it was normalized to the GenBank convention. |
| `WRITE_MOLECULE_TYPE_DERIVED` | info | none | `molecule_type` was absent, so the writer derived it from `kind`. |
| `WRITE_UNSUPPORTED_MOLECULE_TYPE` | error | none | The writer cannot express molecule_type "other" as a legal LOCUS token and refuses to guess. |
| `WRITE_STRAND_LOST` | warning | none | A location with strand 0 was written as a plain interval because GenBank has no unstranded spelling; a re-import will read strand 1. |

## 10. Notes on individual codes

- `LIMIT_EXCEEDED` is the single code for every configured limit. The typed error
  carries the limit name (`max_input_bytes`, `max_records`, `max_sequence_len`,
  `max_features`, `max_segments_per_feature`, `max_qualifier_value_bytes`,
  `max_header_bytes`, `max_location_depth`, `max_snapgene_packets`,
  `max_lzma_output_bytes`, `max_lzma_ratio`, `max_xml_depth`,
  `max_xml_element_bytes`, `max_preserved_block_bytes`), the configured value, the
  observed value, and an optional position. `DNA_XML_DEPTH_EXCEEDED` and
  `DNA_DECOMPRESSION_LIMIT` remain the specific SnapGene codes so that a report
  tells the reader what was too large; a consumer that only cares about limits
  can classify all of them as limit failures.
- `GB_TOPOLOGY_MISSING` and `DNA_TOPOLOGY_MISSING` exist in addition to the
  shared `RECORD_REQUIRED_FIELD_MISSING` because the format-specific code tells
  the reader which field of the source is missing. An adapter emits exactly one
  of the two for a missing topology, never both.
- `RECORD_REQUIRED_FIELD_MISSING` carries the field name in `message` (for
  example `name`) and in the typed error variant, never in the code.
- `GB_LOCATION_BETWEEN_POSITIONS` is informational because the mapping is exact:
  a zero-length segment `start == end` marks the site between the two bases.
  Consumers must not treat it as an empty feature.
- `GB_LOCATION_PARTIAL_BOUNDARY` is informational; the partial markers are kept
  in `start_partial` and `end_partial` and are restored by the writer.
- `GB_LOCUS_LENGTH_MISMATCH` does not invalidate the record: the `ORIGIN` length
  is authoritative and `length` is set from it.
- `EXTRAS_NAMESPACE_COLLISION` is a programming error surfaced as a warning. The
  loaded document keeps the first namespace and the second adapter is not
  allowed to overwrite it; the event is reported so the collision is fixed.
- `DNA_PACKET_IGNORED` and `DNA_UNKNOWN_PACKET` can both accompany a preserved
  block; they answer different questions (known-but-unmapped versus
  unknown-type).
- Writer error codes are carried by `WriteError`. `WRITE_NAME_TRUNCATED`,
  `WRITE_QUOTE_NORMALIZED`, `WRITE_MOLECULE_TYPE_DERIVED` and
  `WRITE_STRAND_LOST` are not errors: they travel in `WriteOutcome.diagnostics`.
  No `WRITE_*` code appears in an import report.
- `DNA_SEQUENCE_EMPTY` is distinct from `DNA_NO_DNA_PACKET`: a DNA packet exists
  but decodes to zero bases. `GB_SEQUENCE_EMPTY` is its GenBank counterpart.
- `DNA_SEQUENCE_NORMALIZED` covers only the uncompressed packet, which can carry
  lowercase letters or whitespace; a 2-bit packet has neither, so it never raises
  this code.
- `DNA_FEATURE_MALFORMED` is paired with a `dropped[]` entry whose `what` is
  `feature` and whose `reason` is `malformed_metadata`; a feature is never
  skipped silently.
- `DNA_HISTORY_DECODE_FALLBACK` records that the version-selected decode failed
  and the documented fallback succeeded. The version rule remains primary; the
  fallback is a safety net, not a replacement.
- `DNA_TOPOLOGY_CONFLICT`: the DNA flags win, and neither the notes nor the
  properties block is rewritten. The conflict is reported so the source can be
  corrected.
- An error-severity entry inside a report always carries `record_index`. An
  import that leaves no usable record returns `ImportError` instead.

## 11. What the schema cannot enforce

The JSON Schema for the import report checks the shape of a diagnostic: `code`
matches `^[A-Z][A-Z0-9_]*$`, `severity` is one of the three values, `message` is
a non-empty string, and a position matches one of the two shapes. It cannot
check:

- that a code exists in this table (the table is versioned separately),
- that the severity in the report equals the severity fixed here,
- which position kind a given code may carry, or that a position is present for a
  code that requires one,
- that `dropped[].reason` belongs to the vocabulary in section 8, or that
  `dropped[].feature_key` and `location_text` are present when `what` is
  `feature`,
- that `codes_version` equals the table version the producer was built with,
- that `message` respects the content rules in section 7.

These are invariants of the adapters and of `Record::validate()`, not of the
schema. They must be covered by adapter tests and by the conformance suite.

## 12. Related documents

- [canonical-model.md](canonical-model.md) - field semantics and decisions
  D1-D16/S1-S9.
- [canonical-json-profile.md](canonical-json-profile.md) - JSON byte rules,
  including the diagnostic sort input and what is not promised.
- `schemas/import-report-1.schema.json` - machine-readable report shape.
- `schemas/examples/report.example.json` - a report using
  `DNA_HISTORY_PRESERVED_RAW` with a SnapGene position and
  `codes_version: 2026-10-21/1`.
