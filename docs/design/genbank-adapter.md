# GenBank adapter

**Status: finalized at M3 (2026-10-21), revised 2026-10-23 (circular
coordinates).** This note is the design
contract for the GenBank reader and writer in `sequon-io`. It adds no code, no
fixtures and no dependencies. The semantic source of truth is
[canonical-model.md](canonical-model.md); the code vocabulary is
[diagnostics.md](diagnostics.md); JSON byte rules are
[canonical-json-profile.md](canonical-json-profile.md). When this note and an
accepted document disagree, the accepted document wins and this note changes in
the same commit.

The reader (M1) and writer (M3) implement this note. The mapping, layout and
diagnostic vocabulary below are frozen; format facts that still need fixture or
corpus confirmation are listed in section 12 and do not change the mapping.

**Revision 2026-10-23: circular coordinates.** The canonical payload stores an
origin-crossing range on a circular record in its unwrapped form. A single
GenBank range such as `5900..6100` on a 6000 bp circular record becomes one
`span` segment `{5899,6100}` whose `end` is greater than `length`; it is not
split and the operator is not rewritten. An explicit source
`join(5900..6000,1..100)` stays two `join` segments. The writer reverses each
form faithfully: an unwrapped span writes `5900..6100`, a split join keeps
`join(5900..6000,1..100)`. Linear records are unchanged and still reject
`end > length`. Sections 5.2-5.4, 5.6, 9.1, 9.6, 9.9, 11 and 13 are updated;
`canonical-model.md` carries the matching model revision and `crates/sequon-model`
owns the topology-aware validation.

## 1. Scope and API surface

### 1.1 What this adapter owns

- Reading GenBank flat files (`.gb`, `.gbk`, `.genbank`, extension irrelevant)
  into `sequon/record/1` records plus one `sequon/import-report/1` report.
- Writing one canonical record as a GenBank flat file, deterministically.
- The `GB_*` diagnostics, the GenBank shape of `dropped[]`, and the
  `WRITE_*` diagnostics.
- Detection of GenBank content for `sequon_io::detect`.

It does not own the canonical types, the digest algorithm, the report schema or
the diagnostic vocabulary; those are the contract role's artifacts.

### 1.2 API entry points served

From development plan section 3.4, with the one approved M0 refinement in
section 1.2.1:

| Entry point | GenBank behaviour |
| --- | --- |
| `detect(bytes, limits) -> Result<Format, ImportError>` | Returns `Format::Genbank` when the input contains a line that starts with `LOCUS` followed by whitespace. A SnapGene cookie (`0x09` packet magic `SnapGene`) wins if present at byte 0. No other heuristic and no extension check. |
| `import(bytes, options) -> Result<ImportOutcome, ImportError>` | Dispatches on `options.format` when set, otherwise on `detect`; `Format::Genbank` calls `import_genbank`. |
| `import_genbank(bytes, options) -> Result<ImportOutcome, ImportError>` | The explicit entry point. It does not consult `options.format`; calling it on SnapGene bytes fails with a GenBank diagnostic (normally `GB_MISSING_LOCUS`). |
| `write_genbank(record, options) -> Result<WriteOutcome, WriteError>` | One record to one GenBank file. See section 1.2.1. |

`ImportOutcome` is `{ records: Vec<Record>, report: ImportReport }`. Warnings
never make the call fail: a lossy feature produces `Ok` with diagnostics and a
`dropped[]` entry. A failure that leaves no usable record returns `Err` and no
report. In a multi-record file, a record-level failure while at least one other
record is usable is an error-severity diagnostic carrying that record's
`record_index`; the usable records are returned (diagnostics.md section 2).
Every `LimitExceeded` is a typed error and aborts the import.

#### 1.2.1 M0 API refinement: `WriteOutcome`

The plan's `write_genbank` returns `Result<Vec<u8>, WriteError>`, which has no
carrier for the non-fatal writer diagnostics `WRITE_NAME_TRUNCATED` (warning),
`WRITE_QUOTE_NORMALIZED` (info), `WRITE_MOLECULE_TYPE_DERIVED` (info) and
`WRITE_STRAND_LOST` (warning). Lead
approved this M0 refinement; it supersedes the section 3.4 signature **only in
the return type**:

```rust
pub struct WriteOutcome {
    pub bytes: Vec<u8>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn write_genbank(record: &Record, options: &WriteOptions) -> Result<WriteOutcome, WriteError>;
```

`Diagnostic` is the same type used by the import report
(`code`, `severity`, `message`, `position`, `record_index`). Writer diagnostics
have no source position (`position: null`, `record_index: null`) and are ordered
by the diagnostic rule in diagnostics.md section 6. `WriteError` variants are owned by the
error module; section 9.1 lists the conditions this adapter must reject, not
the Rust spelling of the variants.

### 1.3 Options consumed

`ImportOptions`:

| Field | GenBank use |
| --- | --- |
| `limits` | Every limit in section 6.5 and section 4.6. Never truncates. |
| `multiple` | `First` / `All` / `Reject`; see section 8. Default `All`. |
| `name_hint` | Used only when the `LOCUS` name is absent or empty. A hint never overrides parsed data. |
| `kind_hint` | Used only when the `LOCUS` molecule token is absent or cannot be classified. A hint never overrides a parsed RNA/protein token. |
| `topology_hint` | Used only when the `LOCUS` topology field is blank. A hint never overrides a parsed topology. |
| `format` | Handled by `import`; ignored by `import_genbank`. |
| `preserve_unhandled_blocks` | No effect on GenBank v1. Unknown header fields go to `extras.genbank.header`, not to `preserved[]`. |

`WriteOptions`:

| Field | GenBank use |
| --- | --- |
| `date` | `DD-MON-YYYY` for the `LOCUS` date. `None` means the fixed placeholder `01-JAN-2000`. The writer never reads a clock. |
| `line_width` | Target width for qualifier lines (default 80). See section 9.5 for the one deliberate exception. |
| `include_header_extras` | `true` restores `extras.genbank.header`; `false` emits only `LOCUS`, a synthesized `DEFINITION` when `description` is present, `FEATURES` and `ORIGIN`. |

### 1.4 M0 boundary

This note is design only. It does not add Rust modules, model types, fixtures,
test data or Cargo entries. Fixture bytes are conformance-owned; section 11
records what each fixture must prove.

### 1.5 Dependencies and IO-1

No third-party GenBank parser may appear in any `Cargo.toml` or `Cargo.lock`:
not `gb-io`, not Biopython bindings, not any equivalent. The reader and writer
are implemented in this repository. The adapter needs only `std`, the
`sequon-model` types and `serde`/`serde_json` already in the workspace; no new
runtime dependency is proposed here. CI asserts the absence with
`cargo tree` / `Cargo.lock` (plan section 7.8, G7). Third-party parsers may be
used as development-time reference and comparison only, never at runtime.

## 2. Reader flow and record boundaries

### 2.1 Pipeline

1. Check `max_input_bytes` before any allocation.
2. Normalize line endings: split the input on LF and strip one trailing CR from
   each line. If at least one CR was stripped, record `GB_CRLF_NORMALIZED`
   (info, no position, once per input).
3. Skip blank lines (whitespace only) until the first `LOCUS`.
4. For each record: parse `LOCUS`, header, `FEATURES` (optional), `ORIGIN`;
   assemble the sequence; validate coordinates against the actual length;
   emit diagnostics; build the `Record` and its report summary.
5. Stop at end of input. A second `LOCUS` after `//` starts the next record.

Features are parsed syntactically before `ORIGIN` is read because that is the
file order. Coordinate validation, circular coordinate normalization and the
skip decisions that need the sequence length happen after `ORIGIN`. Diagnostics
are sorted afterwards (diagnostics.md section 6), so this two-phase order is not
visible in the report.

### 2.2 Input facts

- The reader requires valid UTF-8. Invalid UTF-8 in any text region (header
  value, feature key or location, qualifier, `ORIGIN`) is
  `GB_ENCODING_INVALID` (error, line); v1 never decodes lossily and has no
  latin-1 fallback. Valid non-ASCII text in `ORIGIN` is
  `GB_ORIGIN_INVALID_CHAR`; non-ASCII header and qualifier text is preserved as
  UTF-8.
- A UTF-8 byte order mark before `LOCUS` is non-whitespace content and is
  reported as `GB_CONTENT_BEFORE_LOCUS`.
- CRLF is normalized to LF; a lone CR is not a line ending in v1 and remains in
  the line, where it normally causes a malformed-field diagnostic.
- A missing final newline is accepted. The writer always emits one.
- Extension is never a discriminator. `detect` uses content; a caller that
  knows the format passes `ImportOptions.format = Some(Format::Genbank)`.

### 2.3 Record boundaries and errors

| Condition | Result | Code and position |
| --- | --- | --- |
| No bytes, or only whitespace | typed error | `GB_EMPTY_INPUT`, position none |
| Non-blank content, no `LOCUS` anywhere | typed error | `GB_MISSING_LOCUS`, line = first non-blank line |
| Non-blank content before the first `LOCUS`, with a `LOCUS` later | typed error | `GB_CONTENT_BEFORE_LOCUS`, line = first non-blank line |
| Blank lines before `LOCUS` | accepted | none |
| End of input before a record's `//` | typed error | `GB_RECORD_UNTERMINATED`, line = last line of input |
| A `LOCUS` line inside a record, before its `//` | typed error | `GB_RECORD_UNTERMINATED`, line = the earlier `LOCUS` (a new record cannot start inside a record) |
| Blank lines between records | accepted | none |
| Non-blank content between records | typed error | `GB_CONTENT_BEFORE_LOCUS`, line = the first such line |
| `//` line | record terminator | The line, after right-trimming, must be exactly `//`; leading whitespace is not accepted. |

A file with one record and no trailing newline is valid. A file whose only
content is a `//` line has no `LOCUS`: `GB_MISSING_LOCUS`.

The Result column describes a single-record file. In a multi-record file:
file-level failures (`GB_EMPTY_INPUT`, `GB_MISSING_LOCUS`,
`GB_CONTENT_BEFORE_LOCUS`, `GB_MULTIPLE_RECORDS_NOT_ALLOWED`, every
`LimitExceeded`) always return `Err`; a record-level failure
(`GB_RECORD_UNTERMINATED`, `GB_MALFORMED_LOCUS`, `GB_MISSING_ORIGIN`,
`GB_UNSUPPORTED_MOLECULE`, a malformed location or a malformed `FEATURES`
block) while another record is usable becomes an error-severity diagnostic with
that record's `record_index`, and the usable records are returned. Under
`First`, a failed first record leaves no record to return, so the call returns
`Err`; failures after it are diagnostics.

### 2.4 Section order inside a record

The fixed order is `LOCUS`, header fields, optional `FEATURES`, `ORIGIN`,
`//`.

- `FEATURES` is optional; if `ORIGIN` follows the header directly, the record
  has zero features.
- An `ORIGIN` before `FEATURES` starts the sequence section; any later
  `FEATURES` line is treated as sequence text and fails with
  `GB_ORIGIN_INVALID_CHAR`.
- Any top-level section other than `ORIGIN` after `FEATURES` (for example
  `CONTIG` or `BASE`) is not supported in v1, and the record has no `ORIGIN`:
  `GB_MISSING_ORIGIN` (error). Before `FEATURES`, such a line is an unknown
  header field and is preserved (section 3).
- Missing `ORIGIN` when the record ends at `//`: `GB_MISSING_ORIGIN` (error,
  position none).

### 2.5 Detection details

`detect` returns `Format::Genbank` if any line begins with `LOCUS` followed by
whitespace, even when non-blank content precedes it; that content is then
reported by `import_genbank` as `GB_CONTENT_BEFORE_LOCUS`, which is a more
useful error than "format unknown". If no line matches and there is no SnapGene
cookie, `detect` returns `FormatUnknown` with a short hint that never contains
input data.

## 3. Header mapping

### 3.1 `LOCUS` parsing

The `LOCUS` line is parsed tolerantly by tokens, while keeping the fixed
columns as the primary layout (columns confirmed against current GenBank
records, 2026-10-21):

- The line must start with `LOCUS` at column 1 followed by whitespace;
  otherwise `GB_MALFORMED_LOCUS` (error, line).
- The name is the text between `LOCUS` and the length token, with runs of
  whitespace collapsed to one space and the ends trimmed. If empty, the name is
  absent (hint or `RECORD_REQUIRED_FIELD_MISSING`).
- The length token is the first all-digit token that is followed by `bp`
  (case-insensitive). Missing or overflowing length: `GB_MALFORMED_LOCUS`.
- The molecule token is the token immediately after `bp`; missing token means
  `kind` has no source.
- The topology is read from the fixed field at columns 56-63 when the line is
  long enough, right-justified. If that field is blank, the tokens after the
  molecule are scanned for `linear`/`circular` (older layouts). A non-blank
  field that is neither value is `RECORD_FIELD_INVALID` (error), not "missing".
- The division is the first three-uppercase-letter token between the topology
  and the date token (`DD-MON-YYYY`). It is optional. Stored as
  `extras.genbank.division` when present; omitted when blank.
- The date token is recognized but **not preserved**. The writer uses
  `WriteOptions.date` or `01-JAN-2000` (section 9.3). This is an intentional
  determinism rule, not an unreported loss: the date is output metadata, and
  the plan fixes it.
- Tokens after the date are not part of v1.

Writer layout (columns verified against a current record and the
canonical-json-profile example):

| Columns | Content |
| --- | --- |
| 1-5 | `LOCUS` |
| 6-12 | spaces |
| 13-28 | name, left-justified, 16 characters |
| 29 | space |
| 30-40 | length, right-justified, 11 digits |
| 41 | space |
| 42-43 | `bp` |
| 44-47 | spaces |
| 48-53 | molecule type, left-justified, 6 characters |
| 54-55 | spaces |
| 56-63 | topology, right-justified, 8 characters |
| 64 | space |
| 65-67 | division, 3 letters, or three spaces when absent |
| 68 | space |
| 69-79 | `DD-MON-YYYY` |

The line is 79 columns and never has trailing spaces. With a blank division the
date still starts at column 69, so five spaces separate the topology field from
the date.

### 3.2 Header block parsing

The header block starts after the `LOCUS` line and ends at the `FEATURES` or
`ORIGIN` line (or at `//`, which is then an error because `ORIGIN` is missing).

- A line whose first non-space character is at column 1 starts a new header
  entry. The key is the first whitespace-delimited token. The value starts at
  column 13 for standard keys, or after the key token plus one space when the
  key is longer than 12 characters.
- `ORGANISM` is a special case: it is written indented at column 3 under
  `SOURCE`. A line whose first non-space character is exactly column 3 and
  whose token is `ORGANISM` starts a new `ORGANISM` entry. Every other indented
  line is a continuation of the current entry. This keeps `REFERENCE` subfields
  (`AUTHORS`, `TITLE`, `JOURNAL`, `PUBMED`, `REMARK`) and `COMMENT` text inside
  their parent entry, while giving `ORGANISM` its own ordered entry as the plan
  requires.
- Blank lines inside the header are ignored (they carry no field text).
- Right-trim every physical line.
- Recognized keys: `DEFINITION`, `ACCESSION`, `VERSION`, `KEYWORDS`, `SOURCE`,
  `ORGANISM`, `REFERENCE`, `COMMENT`. `LOCUS`, `FEATURES` and `ORIGIN` are
  structural and are not header entries.
- Any other top-level key is preserved in `extras.genbank.header` in source
  order and reported once per distinct key with `GB_HEADER_FIELD_UNKNOWN`
  (info, line of first occurrence). Indented unknown lines are continuations
  and therefore also preserved, inside their parent value.
- Header keys are matched case-insensitively for recognition but stored as
  written. Duplicate keys are kept, in order (multiple `REFERENCE` entries are
  normal).
- `max_header_bytes` is checked incrementally over the normalized header lines;
  exceeding it is `LimitExceeded` with limit name `max_header_bytes`.

### 3.3 `extras.genbank.header` representation

The header is an ordered array of `{ "key": string, "value": string }`.

- `key` is the token as written; `ORGANISM` is stored without its leading
  spaces.
- `value` is a single string. The first element is the text after the value
  column, right-trimmed. Each continuation line is appended as `"\n"` plus the
  entire physical line, right-trimmed and **including its original leading
  whitespace**. This preserves `REFERENCE` subfield indentation and taxonomy
  indentation exactly, and makes the writer's restoration unambiguous.
- Canonical JSON writes those line feeds as `\n` escapes; the value is still
  one JSON string (JSON profile section 6).
- The writer emits the first line as `key` padded to column 13 followed by the
  first value line, then emits every continuation line verbatim. A continuation
  line that is non-empty and does not begin with whitespace is prefixed with 12
  spaces so it cannot be mistaken for a new header key. Trailing whitespace is
  never emitted.
- `DEFINITION` is stored in the header like every other field and also
  populates `description` (section 3.4).
- `ORGANISM` does **not** create a second `extras.genbank.organism` field in
  v1. The header is authoritative; a convenience copy would be a second store.
- The `LOCUS` division token is stored separately as
  `extras.genbank.division` (string, omitted when blank), per the accepted
  model. It is not part of the header array.

Example of the raw-continuation rule:

```text
REFERENCE   1  (bases 1 to 120)
  AUTHORS   Doe,J. and Roe,R.
  TITLE     A synthetic example
  JOURNAL   Unpublished
```

The stored `REFERENCE` value is
`"1  (bases 1 to 120)\n  AUTHORS   Doe,J. and Roe,R.\n  TITLE     A synthetic example\n  JOURNAL   Unpublished"`.
The writer reproduces the indentation without reflowing it.

### 3.4 `DEFINITION` and `description`

`description` is the `DEFINITION` value with line feeds replaced by spaces,
runs of ASCII whitespace collapsed to one space, and the ends trimmed. If the
result is empty, `description` is `null`. If `DEFINITION` is absent,
`description` is `null`; that is not an error. If several `DEFINITION` entries
exist, the first supplies `description`; all are preserved in the header.

The writer prefers the header `DEFINITION` (round-trip fidelity). Only when the
header has no `DEFINITION` does it synthesize one from `description`. If a
consumer edits `description` without updating the header, the header value wins
on write; this is deliberate and is recorded here so it is not a surprise. A
future edit API should synchronize both.

### 3.5 Header mapping table

| GenBank field | Canonical destination | Rule |
| --- | --- | --- |
| `LOCUS` name | `name` | As written (whitespace-normalized). Empty/absent falls back to `name_hint`; both absent is `RECORD_REQUIRED_FIELD_MISSING { field: "name" }` (error). |
| `LOCUS` length | `length` check | If it differs from the actual `ORIGIN` length, `GB_LOCUS_LENGTH_MISMATCH` (warning); the actual length wins. |
| `LOCUS` molecule | `kind`, `molecule_type` | Table in section 3.6. RNA/protein fail; unknown DNA subtype becomes `other` with `MOLECULE_TYPE_UNKNOWN`. |
| `LOCUS` topology | `topology` | `linear`/`circular`; blank falls back to `topology_hint`; both absent is `GB_TOPOLOGY_MISSING` (error); a non-blank unknown value is `RECORD_FIELD_INVALID` (error). |
| `LOCUS` division | `extras.genbank.division` | Optional string; omitted when blank. Not a header entry. |
| `LOCUS` date | not preserved | Writer date comes from `WriteOptions.date` or `01-JAN-2000`. |
| `DEFINITION` | `description` and `extras.genbank.header` | Header entry kept in source order; `description` is the normalized text. |
| `ACCESSION` | `extras.genbank.header` | Ordered entry; never synthesized on write. |
| `VERSION` | `extras.genbank.header` | Ordered entry. |
| `KEYWORDS` | `extras.genbank.header` | Ordered entry; value kept as written (often `.`). |
| `SOURCE` | `extras.genbank.header` | Ordered entry. |
| `ORGANISM` | `extras.genbank.header` | Ordered entry; taxonomy continuation lines are part of its value. |
| `REFERENCE` | `extras.genbank.header` | One entry per reference; subfields are continuation lines. |
| `COMMENT` | `extras.genbank.header` | Ordered entry; multi-line text preserved. |
| Unknown top-level header key | `extras.genbank.header` | Preserved in place plus `GB_HEADER_FIELD_UNKNOWN` (info, once per distinct key). |
| `FEATURES`, `ORIGIN` | structural | Not header entries. |

### 3.6 Molecule token to `kind` and `molecule_type`

Token comparison is case-insensitive; `kind` is only ever `dna` for a
successful v1 import.

| `LOCUS` molecule token | `kind` | `molecule_type` | Outcome |
| --- | --- | --- | --- |
| `DNA`, `ds-DNA` | `dna` | `ds-DNA` | success |
| `ss-DNA` | `dna` | `ss-DNA` | success |
| any other token ending in `DNA` (for example `ms-DNA`) | `dna` | `other` | success + `MOLECULE_TYPE_UNKNOWN` (warning, line) |
| `mRNA`, `RNA`, `pre-mRNA`, `rRNA`, `tRNA`, `uRNA`, `ncRNA`, `scRNA`, `ss-RNA`, `ds-RNA`, `ms-RNA` | `rna` | not produced | `GB_UNSUPPORTED_MOLECULE` (error, line) |
| `protein`, `PRT`, `AA` | `protein` | not produced | `GB_UNSUPPORTED_MOLECULE` (error, line) |
| absent or empty | from `kind_hint`, otherwise no source | `null` | `RECORD_REQUIRED_FIELD_MISSING { field: "kind" }` when no hint |
| any other token | from `kind_hint`, otherwise no source | `null` | `RECORD_REQUIRED_FIELD_MISSING { field: "kind" }` when no hint |

Rules:

- A parsed non-DNA token always wins over `kind_hint`; the result is
  `GB_UNSUPPORTED_MOLECULE`. Hints never downgrade or override input.
- When `kind_hint = dna` supplies the missing kind, `molecule_type` stays
  `null`; the writer later derives `ds-DNA` with `WRITE_MOLECULE_TYPE_DERIVED`.
- `/mol_type` is a feature qualifier. It does **not** set `molecule_type`;
  only the `LOCUS` token does.
- `LOCUS` division and date never affect `kind`, `molecule_type` or
  `topology`.

### 3.7 Required fields (D16)

| Field | Source order | Failure |
| --- | --- | --- |
| `name` | `LOCUS` name, then `name_hint` | `RECORD_REQUIRED_FIELD_MISSING` (error, line/offset of `LOCUS`), field `name` |
| `kind` | `LOCUS` molecule, then `kind_hint` | `RECORD_REQUIRED_FIELD_MISSING`, field `kind`; parsed RNA/protein is `GB_UNSUPPORTED_MOLECULE` |
| `topology` | `LOCUS` topology, then `topology_hint` | `GB_TOPOLOGY_MISSING` (error, line); a non-blank unknown value is `RECORD_FIELD_INVALID` |

`GB_TOPOLOGY_MISSING` and `RECORD_REQUIRED_FIELD_MISSING` are never both
emitted for GenBank topology; the format-specific code is used, and its typed
error names the field. Empty hints are treated as absent; a non-empty hint is
used as written.

## 4. FEATURES block

### 4.1 Boundaries

- The `FEATURES` heading is the top-level line whose first token is `FEATURES`.
  Its trailing text (`Location/Qualifiers`) is structural and is not stored.
- The block ends at the next line whose first non-space character is at column
  1: normally `ORIGIN`; `//` gives a zero-feature record that then fails with
  `GB_MISSING_ORIGIN`; any other top-level section is treated as described in
  section 2.4.
- Feature blocks are kept in source order. `features` is never sorted.

### 4.2 Line classification priority

Apply these rules in order to every line after the `FEATURES` heading:

1. **Qualifier mark.** If the first non-space character is `/`, the line starts
   a new qualifier on the current feature. This test precedes the column test:
   no feature key begins with `/`, and a qualifier can drift left without
   becoming a feature. The precedence was approved by lead on 2026-10-21
   (interpretation of plan section 5.3).
2. **New feature.** Otherwise, if the first non-space character is in columns
   1-20, the line starts a new feature. The key field is columns 6-20; when the
   first non-space is left of column 6, the key starts there instead (tolerant
   recovery). The location text is the remainder of the line after the key
   field, trimmed; when the key exceeds column 20, it is the remainder after
   the key token. A line with no key text in the key field is a structurally
   broken block: `GB_FEATURES_MALFORMED` (error, line).
3. **Continuation.** Otherwise (first non-space at column 21 or later) the line
   continues the current feature: the last qualifier when one exists, otherwise
   the feature's location text. A continuation with no current feature is a
   structurally broken block.

Column drift (`first non-space != 6` for a new feature) is tolerated by these
rules and reported once per input with `GB_FEATURE_COLUMN_MISALIGNED` (info,
line and column of the first drifted feature). The standard columns are:
feature key at 6-20, location from 22, qualifiers from 22, continuation lines
indented 21 spaces.

### 4.3 Qualifier grammar

The raw qualifier text is assembled from the line starting with `/` plus any
continuation lines, then parsed:

| Form | Result | Diagnostic |
| --- | --- | --- |
| `/key` | `value: null` (bare qualifier, e.g. `/pseudo`) | none |
| `/key=value` | `value: "value"` exactly as written | `GB_QUALIFIER_UNQUOTED` (info, line) |
| `/key=""` | `value: ""` | none |
| `/key="value"` | `value: "value"` with the outer quotes removed; no escape processing | none |
| `/key="unterminated` | value kept as the raw text including the opening quote; the record stays usable | `GB_QUALIFIER_UNQUOTED` (info) with a message that the quote is not closed |
| `/key=` (empty, no quotes) | `value: ""` | `GB_QUALIFIER_UNQUOTED` (info) |

Details:

- The key is the text up to the first `=` (trimmed); without `=` the whole
  remainder is the key. Keys are not validated against a whitelist and are never rewritten.
- Quotes are not escaped on read. A `"` inside a quoted value remains a
  literal character. (The writer's replacement rule is in section 9.5; the
  round-trip consequence is documented there.)
- Duplicate keys are preserved in order (D3). `/db_xref` and `/note` are the
  common cases; the array is the only qualifier store.
- Values are not semantically rewritten. `/codon_start`, `/transl_table`,
  `/translation`, `/product`, `/gene` and `/note` stay strings.
- `max_qualifier_value_bytes` is checked against the assembled value in UTF-8
  bytes; exceeding it is `LimitExceeded` with limit name
  `max_qualifier_value_bytes`.

### 4.4 Continuation lines

- The 21-column indent is structural and is removed; any additional leading
  whitespace beyond the indent is also removed for qualifier and location
  continuation text. Each line is right-trimmed.
- Continuation text is appended to the current qualifier or location with a
  single ASCII space. For locations this is safe because the grammar ignores
  whitespace between tokens. For quoted qualifier values the line break becomes
  a single space, which is the normal GenBank interpretation of display
  wrapping.
- A continuation after a bare qualifier (`/pseudo`) is structurally broken:
  the bare qualifier has no value to continue; `GB_FEATURES_MALFORMED` (error,
  line).
- The writer never wraps a feature's key/location line, so a generated file
  always gives the reader an unambiguous location on the feature line. A
  location wrapped across lines in third-party input is still accepted through
  rule 3 and is an open format fact (section 12).

### 4.5 Feature assembly and order

Per feature:

1. `key` from the key field, kept verbatim (case-sensitive, no rewriting to
   `misc_feature`).
2. `name` from the first `/gene` qualifier with a non-empty string value;
   otherwise `null`. Never from `/product`, never synthesized
   (canonical-model.md, Features).
3. `location` from the location text, parsed per section 5, then validated
   against the sequence length after `ORIGIN`.
4. `qualifiers` in source order, duplicates kept.
5. `so_term` from the static table in section 7; unknown key is `null` plus
   `GB_UNKNOWN_FEATURE_KEY` (info, line).

Features are emitted in source order. A skipped feature is absent from
`features`, present in `dropped[]`, and explained by one warning (section 4.6).

### 4.6 D11: skip versus abort

Structural damage aborts the record (and the import when no usable record
remains); an inexpressible feature is skipped and counted. The exact split:

| Situation | Result | Diagnostic | `dropped[]` |
| --- | --- | --- | --- |
| Unsupported construct: `one-of`, `replace`, remote accession, `telomere`, `gap` | skip the feature | `GB_LOCATION_UNSUPPORTED` (warning, line) | `what: "feature"`, `reason: "unsupported_location"`, `feature_key`, `location_text`, `record_index`, `position` |
| Mixed-strand or nested-operator location that one `strand` plus one operator cannot represent | skip the feature | `GB_LOCATION_UNSUPPORTED` (warning, line) | same as above |
| Linear coordinate beyond the record length (but within `max_sequence_len`) | skip the feature | `GB_COORDINATE_OUT_OF_RANGE` (warning, line) | `what: "feature"`, `reason: "coordinate_out_of_range"`, `feature_key`, `location_text`, `record_index`, `position` |
| Circular feature longer than the sequence | skip the feature | `GB_COORDINATE_OUT_OF_RANGE` (warning, line) | same |
| Syntactically malformed location (unbalanced parentheses, non-numeric bound, unknown operator, zero or reversed bound, invalid nesting) | abort the record | `GB_LOCATION_MALFORMED` (error, line + column) | record omitted; if no usable record remains, `Err` with no report |
| Coordinate value beyond `max_sequence_len` | abort the import | `LimitExceeded`, limit `max_sequence_len` | no report |
| `max_features`, `max_segments_per_feature`, `max_location_depth`, `max_qualifier_value_bytes`, `max_header_bytes` | abort the import | `LimitExceeded` with the limit name | no report |
| Structurally broken `FEATURES` block (stray qualifier or continuation, empty key, bare qualifier followed by a continuation) | abort the record | `GB_FEATURES_MALFORMED` (error, line) | record omitted; if no usable record remains, `Err` with no report |

Skipping is per feature, never per segment: if one segment of a `join` is out
of range, the whole feature is skipped, because dropping that segment would
silently change the annotation. `dropped[]` entries carry only identity and
reason, never payload bytes; a GenBank feature has no raw byte span worth
copying, so `digest` is not emitted. The `feature` kind requires `feature_key`
and `location_text` (diagnostics.md section 8).

When several features are skipped, each gets its own warning and dropped entry.
The record itself remains valid. Record-level errors are `Err` only when no
usable record remains; otherwise they are error-severity diagnostics in the
report (sections 2.3 and 8). `LimitExceeded` always aborts the import because
`LIMIT_EXCEEDED` is not a report diagnostic.

### 4.7 FEATURES limits

- `max_features` counts feature blocks as they are assembled.
- `max_segments_per_feature` counts flattened segments of one location after
  parsing and before conversion.
- `max_location_depth` counts nesting of `complement`/`join`/`order`.
- `max_qualifier_value_bytes` counts each assembled value.
All are `LimitExceeded`, never truncation.

## 5. Location grammar

### 5.1 Coordinate conversion

GenBank locations are 1-based closed. Canonical segments are 0-based
half-open:

```text
GenBank 123..456   ->  { start: 122, end: 456 }
GenBank 123        ->  { start: 122, end: 123 }
GenBank 100^101    ->  { start: 100, end: 100 }   // zero-length site
```

Conversion happens once, here. Segment order is the written order for `join`
and `order`; `order` is never sorted.

### 5.2 Grammar mapping

| GenBank | Canonical |
| --- | --- |
| `123..456` | `operator: "span"`, `strand: 1`, one segment `{122,456}` |
| `5900..6100` on a 6000 bp circular record | `operator: "span"`, one unwrapped segment `{5899,6100}` (`end > length`) |
| `123` | `operator: "span"`, one segment `{122,123}` |
| `complement(123..456)` | `operator: "span"`, `strand: -1`, segment `{122,456}` unchanged |
| `join(1..10,20..30)` | `operator: "join"`, two segments in written order |
| `join(5900..6000,1..100)` | `operator: "join"`, two segments; an explicit split stays split |
| `complement(join(1..10,20..30))` | `operator: "join"`, `strand: -1`, same segment order |
| `order(1..10,20..30)` | `operator: "order"`, segments in written order, never sorted |
| `join(1..10)` | `operator: "join"` with one segment (operator preserved) |
| `<1..>100` | segment `{0,100}` with `start_partial: true`, `end_partial: true` |
| `100^101` | zero-length segment `{100,100}`, `GB_LOCATION_BETWEEN_POSITIONS` (info) |
| `one-of(...)`, `replace(...)`, `ACC.1:1..100`, `telomere(...)`, `gap(...)` | unsupported: skip feature + `GB_LOCATION_UNSUPPORTED` + dropped |
| `complement(complement(...))`, `<` on an end, `>` on a start, `n^m` with `m != n+1` | malformed: `GB_LOCATION_MALFORMED` (error) |

### 5.3 Nesting and strand

- `complement` is accepted only around the whole location or around a single
  range. `complement` inside a `join`/`order` (per-segment orientation) is not
  representable by one `strand` without losing biological order, so the feature
  is skipped with `GB_LOCATION_UNSUPPORTED`.
- Nested `join` inside `join` (or `order` inside `order`) is flattened in
  written order. A nested operator change (`join` inside `order` or the
  reverse) is not representable by one `operator`; skip with
  `GB_LOCATION_UNSUPPORTED`.
- `span` always has exactly one segment. On a circular record a `span` that
  crosses the origin is stored unwrapped as that one segment: `end` is greater
  than `length` (at most one wrap) and `operator` stays `span`. A `join` or
  `order` keeps its operator and its written segments; an explicitly split
  source across the origin stays split and is never merged.
- `strand` is set only by `complement`; GenBank never produces `0`.
- Partial markers belong to the segment boundary where they are written. They
  are not swapped by `complement`: `complement(<1..>100)` keeps
  `start_partial` on `{0,...}` and `end_partial` on `{...,100}`.
- A `>` on the start or a `<` on the end is malformed. A single position may
  carry one marker (`<100` or `>100`) or, in a range, both.
- `n^m` must have `m == n+1`; it becomes `{n,n}`. `n` must be at least 1.

### 5.4 Circular coordinates (D2, revised 2026-10-23)

For a circular record of length `L`, a raw 1-based closed segment
`[a,b]` becomes 0-based half-open `start = a-1`, `end = b`:

- `span = end - start` (number of bases).
- If `span > L`, the feature cannot be represented: skip with
  `GB_COORDINATE_OUT_OF_RANGE`.
- Otherwise let `s = start % L` and store ONE segment `{s, s+span}`. `end` may
  therefore exceed `L` by at most `L-1`; that is the unwrapped form and the
  payload keeps it.
- `start_partial` and `end_partial` stay on that one segment; there is no
  internal boundary to rewrite.
- A `join` or `order` keeps its written segments; each segment is normalized by
  the same rule and the segments are neither split nor merged. An explicitly
  split source across the origin, such as `join(5900..6000,1..100)`, therefore
  stays two segments even though it is contiguous on the circle.
- The canonical payload keeps the source form. An unwrapped span and a split
  join are both valid representations of the same bases; the model's
  `Segment::resolved` and `Location::resolved_segments` accessors resolve both
  to origin-ordered pieces for consumers that need them.

Worked arithmetic (revised rule):

```text
L = 6000, input 5900..6100 (1-based closed)
start = 5899, end = 6100, span = 201, s = 5899
span <= L -> one segment {5899,6100}, operator = span, strand = 1
writer: 5900..6100                 // 201 bases, no loss, not split
explicit join(5900..6000,1..100)   -> {5899,6000}, {0,100}, operator = join
writer: join(5900..6000,1..100)    // 101 + 100 = 201 bases, no loss
```

A linear record never wraps. `end > L` or `start >= L` on a linear record is
`GB_COORDINATE_OUT_OF_RANGE` (warning, feature skipped); a linear record never
contains `end > length`.

For circular input whose `start` is already beyond `L`, the modulo rule above
wraps the start and keeps the span; this is accepted for tolerance but is an
open fixture fact (section 12). Coordinates beyond `max_sequence_len` are a
`LimitExceeded`, not a coordinate-out-of-range warning (lead decision,
2026-10-21).

Between-positions sites are not wrapped: `n^m` becomes `{n,n}`. On a linear
record `n < L` (a right-hand base must exist); on a circular record
`n <= L` (`L^L+1` is the origin site). A site outside that range is skipped
with `GB_COORDINATE_OUT_OF_RANGE`.

### 5.5 Location failure classes

| Class | Examples | Result |
| --- | --- | --- |
| Malformed syntax | unbalanced parentheses; non-numeric bound; `0`; `12..10`; `>` on a start; `<` on an end; unknown operator; empty list element; `n^m` non-adjacent; nested `complement` | error, `GB_LOCATION_MALFORMED`, line + column |
| Unsupported construct | `one-of`, `replace`, remote accession, `telomere`, `gap`; per-segment `complement`; mixed nesting | skip feature, warning `GB_LOCATION_UNSUPPORTED`, dropped entry |
| Out of range | linear `end > L` or `start >= L`; circular span `> L`; between site outside its range | skip feature, warning `GB_COORDINATE_OUT_OF_RANGE`, dropped entry |
| Limit | `max_location_depth`, `max_segments_per_feature`, coordinate value `> max_sequence_len` | typed `LimitExceeded`, no report |

### 5.6 Parsing algorithm

1. Tokenize the location text, ignoring ASCII whitespace between tokens.
2. Parse a recursive expression:
   `complement ( location )`, `join ( list )`, `order ( list )`, a range with
   optional `<`/`>` markers, or a between site.
3. Recognized unsupported operators (`one-of`, `replace`, `telomere`, `gap`)
   and remote locations (an accession followed by `:`) mark the feature
   unsupported and stop parsing it.
4. Any other identifier followed by `(` is malformed.
5. Count nesting depth and leaves while parsing; exceed a limit immediately.
6. Flatten same-operator nesting, preserving order; reject per-segment
   complement and operator changes.
7. Emit the canonical location with `operator`, `strand` and segments; defer
   length validation and circular coordinate normalization until `ORIGIN` has
   been read.
8. Malformed input reports the 1-based line and column of the offending token.

## 6. ORIGIN and sequence

### 6.1 Section

- Starts at the top-level line whose first token is `ORIGIN`.
- Ends at a line that is exactly `//` after right-trimming.
- Every line between is sequence data. Blank lines are ignored.
- If `//` is reached without `ORIGIN`, `GB_MISSING_ORIGIN` (error, no position).
- If end of input is reached first, `GB_RECORD_UNTERMINATED` (error, line).

### 6.2 Line processing

For each sequence line:

1. Right-trim it. Skip if empty.
2. Strip leading spaces/tabs. If the first token is all ASCII digits, strip that
   token and the following whitespace. This is the line number.
3. Process the remaining characters from left to right:
   - ASCII space or tab: ignored.
   - `A`, `C`, `G`, `T`, `N`, `R`, `Y`, `S`, `W`, `K`, `M`, `B`, `D`, `H`,
     `V`: appended.
   - `U` or `u`: error `GB_RNA_BASE_IN_DNA` (line of the character).
   - lowercase equivalents of the accepted letters (never `u`): uppercased and
     appended; set the record's normalization flag.
   - anything else (digit inside the base area, punctuation, non-ASCII):
     error `GB_ORIGIN_INVALID_CHAR` (line of the character).
4. Append to the record sequence. Before appending, check
   `max_sequence_len`; crossing it is `LimitExceeded` with limit
   `max_sequence_len` and the observed length. Never truncate.

Digits after the line number are **not** silently skipped: they are invalid
sequence characters. Only the leading line-number token is structural.

### 6.3 Diagnostics and normalization

- `GB_SEQUENCE_NORMALIZED` (info, no position) is emitted once per record when
  lowercase bases are uppercased. `ORIGIN` line numbers, grouping spaces and
  line breaks are structural formatting, not a normalization event
  (diagnostics.md).
- `GB_CRLF_NORMALIZED` is once per input, before any record.
- `GB_ORIGIN_INVALID_CHAR` and `GB_RNA_BASE_IN_DNA` abort the record; the first
  offending character in reading order is reported. Invalid UTF-8 is
  `GB_ENCODING_INVALID` (line) and is checked before character classification.
- `GB_SEQUENCE_EMPTY` (error, no position) when the `ORIGIN` section produced
  zero bases.
- `GB_LOCUS_LENGTH_MISMATCH` (warning, `LOCUS` line) when the declared length
  differs from the actual base count. The actual count becomes `length`.
- Messages never contain sequence content.

### 6.4 IUPAC set

The accepted DNA alphabet is `ACGTNRYSWKMBDHV`, uppercase, no whitespace.
`U` is not a DNA base; it is a specific error, not a generic invalid character.
RNA/protein records fail earlier at `LOCUS` (`GB_UNSUPPORTED_MOLECULE`), so a
DNA record containing `U` is malformed DNA.

### 6.5 GenBank limits

| Limit | Checked |
| --- | --- |
| `max_input_bytes` | before parsing, whole input |
| `max_records` | at each `LOCUS` start |
| `max_sequence_len` | while appending `ORIGIN` bases; also rejects a coordinate value above it |
| `max_features` | while assembling feature blocks |
| `max_segments_per_feature` | after flattening one location |
| `max_qualifier_value_bytes` | when assembling one qualifier value |
| `max_header_bytes` | while reading header lines |
| `max_location_depth` | during location parsing |

All are `LimitExceeded` and abort; nothing is truncated. SnapGene-specific
limits do not apply.

## 7. Feature semantics and SO mapping

### 7.1 Key, name, qualifiers

- `key` is kept verbatim. Unknown keys are not rewritten.
- `so_term` is derived only from the static table below; anything not in the
  table is `null`, with `GB_UNKNOWN_FEATURE_KEY` (info). The table is
  case-sensitive (`CDS` maps; `cds` does not) and is not a general GenBank
  vocabulary check; it is the adapter's known set.
- `name` is the first `/gene` qualifier with a non-empty string value, else
  `null`. The `/gene` qualifier itself stays in `qualifiers`. Never `/product`,
  never synthesized.
- Qualifier values are strings or `null`, in source order, duplicates kept.
  No semantic rewriting of `/translation`, `/product`, `/gene`, `/note`,
  `/codon_start` or `/transl_table`.
- Colors, display labels and other UI-only attributes are not part of GenBank
  and are not created here.

### 7.2 SO term table (v1)

Proposal: a static table of 44 common GenBank feature keys. The SO identifiers
were checked on 2026-10-21 against the Sequence Ontology OBO release
`2026-08-07`. Basis values: `INSDC EXACT` means SO declares
`INSDC_feature:<key>` as an EXACT synonym; `INSDC qualifier` means the key is
the legacy spelling of an INSDC `regulatory`/other class whose qualifier SO
declares EXACT; `SO name` means the SO term name matches, with no INSDC exact
synonym; `INSDC BROAD` means SO declares a BROAD association. An unknown key is
`so_term: null`; the table is never extended by guessing at import time.

| GenBank key | SO term | SO ID | Basis |
| --- | --- | --- | --- |
| `source` | sequence_source | SO:0002206 | INSDC EXACT |
| `gene` | gene | SO:0000704 | INSDC EXACT |
| `CDS` | CDS | SO:0000316 | INSDC EXACT |
| `mRNA` | mRNA | SO:0000234 | INSDC EXACT |
| `tRNA` | tRNA | SO:0000253 | INSDC EXACT |
| `rRNA` | rRNA | SO:0000252 | INSDC EXACT |
| `ncRNA` | ncRNA | SO:0000655 | SO name |
| `tmRNA` | tmRNA | SO:0000584 | INSDC EXACT |
| `misc_RNA` | transcript | SO:0000673 | INSDC BROAD |
| `promoter` | promoter | SO:0000167 | SO name |
| `terminator` | terminator | SO:0000141 | SO name |
| `enhancer` | enhancer | SO:0000165 | SO name |
| `silencer` | silencer | SO:0000625 | SO name |
| `insulator` | insulator | SO:0000627 | SO name |
| `RBS` | ribosome_entry_site | SO:0000139 | INSDC qualifier |
| `polyA_signal` | polyA_signal_sequence | SO:0000551 | INSDC qualifier |
| `polyA_site` | polyA_site | SO:0000553 | INSDC EXACT |
| `TATA_signal` | TATA_box | SO:0000174 | INSDC qualifier |
| `-35_signal` | minus_35_signal | SO:0000176 | INSDC qualifier |
| `-10_signal` | minus_10_signal | SO:0000175 | INSDC qualifier |
| `GC_signal` | GC_rich_promoter_region | SO:0000173 | INSDC qualifier |
| `CAAT_signal` | CAAT_signal | SO:0000172 | SO name |
| `primer_bind` | primer_binding_site | SO:0005850 | INSDC EXACT |
| `protein_bind` | protein_binding_site | SO:0000410 | INSDC EXACT |
| `regulatory` | regulatory_region | SO:0005836 | INSDC BROAD |
| `rep_origin` | origin_of_replication | SO:0000296 | INSDC EXACT |
| `oriT` | oriT | SO:0000724 | INSDC EXACT |
| `LTR` | long_terminal_repeat | SO:0000286 | INSDC qualifier |
| `repeat_region` | repeat_region | SO:0000657 | INSDC BROAD |
| `mobile_element` | mobile_genetic_element | SO:0001037 | INSDC EXACT |
| `sig_peptide` | signal_peptide | SO:0000418 | INSDC EXACT |
| `transit_peptide` | transit_peptide | SO:0000725 | INSDC EXACT |
| `mat_peptide` | mature_protein_region | SO:0000419 | INSDC EXACT |
| `propeptide` | propeptide | SO:0001062 | INSDC EXACT |
| `misc_feature` | sequence_feature | SO:0000110 | INSDC EXACT |
| `variation` | sequence_alteration | SO:0001059 | INSDC EXACT |
| `operon` | operon | SO:0000178 | INSDC EXACT |
| `intron` | intron | SO:0000188 | INSDC EXACT |
| `exon` | exon | SO:0000147 | INSDC EXACT |
| `5'UTR` | five_prime_UTR | SO:0000204 | INSDC EXACT |
| `3'UTR` | three_prime_UTR | SO:0000205 | INSDC EXACT |
| `assembly_gap` | gap | SO:0000730 | INSDC NARROW |
| `centromere` | centromere | SO:0000577 | INSDC EXACT |
| `telomere` | telomere | SO:0000624 | INSDC EXACT |

Notes:

- `sig_peptide`/`transit_peptide`/`mat_peptide`/`propeptide` also have
  `*_region_of_CDS` terms with EXACT INSDC synonyms; v1 uses the generic region
  term for stability. Choosing the other spelling is a table change, not a
  parser behaviour change.
- `RBS` maps to `ribosome_entry_site` (the INSDC `ribosome_binding_site` class),
  not to the narrower `Shine_Dalgarno_sequence`.
- `misc_RNA`, `regulatory` and `repeat_region` use INSDC BROAD associations;
  they are marked BROAD in the table and are the first rows to review if the
  table is tightened.
- `so_term` is import-only. The writer ignores it and GenBank output has no SO
  field. A table change affects future imports, not digests.
- Maintenance: change the table and this note together; no network access at
  build or run time.

## 8. Multi-record policy

| Policy | Behaviour |
| --- | --- |
| `All` (default) | Return every usable record, in source order, up to `max_records`; a failed record is an error-severity diagnostic. |
| `First` | Return the first record when it is usable. The remaining records are still parsed to produce diagnostics; a failure after the first is a diagnostic, not a failed call. |
| `Reject` | As soon as a second `LOCUS` would start, return `GB_MULTIPLE_RECORDS_NOT_ALLOWED` (error, line of the second `LOCUS`). |

- `record_index` is the 0-based parse-order ordinal, not necessarily an offset
  into `records[]`. Diagnostics, dropped and (SnapGene) preserved entries use
  it to identify the record. `records[]` lists only returned records; under
  `First`, diagnostics for non-returned records may carry an index with no
  `records[]` entry (diagnostics.md section 2).
- A failure that leaves no usable record returns `Err` and no report. In a
  multi-record file, a record-level failure while another record is usable is an
  error-severity diagnostic with the failed record's `record_index`, and the
  usable records are returned. Under `First`, if the first record fails, no
  record is returned, so the call returns `Err`; failures after it are
  diagnostics. Lossy features do not abort.
- `max_records` is checked at each `LOCUS` start; exceeding it is
  `LimitExceeded`, which always returns `Err` and no report (`LIMIT_EXCEEDED` is
  not a report diagnostic).
- A report never mixes formats and never mixes positions from two formats.

## 9. Writer

### 9.1 Preconditions and kind restriction

`write_genbank` accepts one canonical record and returns `WriteOutcome`. It
rejects, with a typed `WriteError`, any record that violates the canonical
contract or cannot be represented in GenBank:

- `kind` other than `dna`: `WRITE_UNSUPPORTED_KIND` (error, no position).
- Empty `name`; empty or non-IUPAC `sequence`; `length` not equal to the
  sequence length; invalid `topology`.
- A segment with `start > end`; on a linear record `end > length`; on a
  circular record `start >= length` or `end > start + length` (more than one
  wrap); a `span` with more than one segment; a zero-length segment at
  `start == 0`; or partial flags on a zero-length segment.
- `molecule_type == "other"`: the writer cannot express it as a legal `LOCUS`
  token and refuses to guess. It returns `WRITE_UNSUPPORTED_MOLECULE_TYPE`
  (error) in `WriteError`; no raw token is preserved in v1.
- An invalid `WriteOptions.date`, an invalid `extras.genbank.division`
  (must be exactly three uppercase ASCII letters when present), or a malformed
  header entry.

`WriteError` is owned by the error module. The conditions above are the
adapter's contract; no new diagnostic code is invented. One case is not a
rejection: `strand == 0` is written as a plain interval and reported with
`WRITE_STRAND_LOST` in `WriteOutcome.diagnostics` (section 9.6).

### 9.2 Output shape and determinism

- Output is UTF-8/ASCII text with LF line endings and exactly one final
  newline after the `//` line.
- Order: `LOCUS`, header entries in `extras.genbank.header` order (with a
  synthesized `DEFINITION` when needed), `FEATURES`, `ORIGIN`, `//`.
- Features in canonical order; qualifiers in array order; segments in array
  order. Nothing is sorted, merged or deduplicated.
- No wall clock, locale, environment variable, hash-map order or random value
  affects output. The same record and options produce byte-identical output.
- `LOCUS` date: `WriteOptions.date` when present and valid, otherwise
  `01-JAN-2000`. The source date is not restored; this is intentional.
- `extras.genbank.division` is restored at columns 65-67 when present and
  valid; otherwise the field is three spaces.

### 9.3 `LOCUS` line

Use the column table in section 3.1. Details:

- Name: at most 16 characters. If longer, emit the first 16 characters and a
  `WRITE_NAME_TRUNCATED` (warning) diagnostic with the original character
  count. The name is not escaped.
- Length: decimal, right-justified in 11 columns; overflow cannot happen for a
  valid record within limits, but if it would, the writer rejects.
- Molecule token: `ds-DNA` -> `DNA`; `ss-DNA` -> `ss-DNA`; `null` ->
  `WRITE_MOLECULE_TYPE_DERIVED` (info) and `DNA`. `other` is rejected with
  `WRITE_UNSUPPORTED_MOLECULE_TYPE` (error); the writer does not guess a legal
  token. `DNA` and `ss-DNA` are the legal INSDC tokens used by current records;
  both re-read as the corresponding canonical value.
- Topology: `linear` or `circular`, right-justified in 8 columns.
- Division: three uppercase letters from `extras.genbank.division`, or three
  spaces.
- Date: `DD-MON-YYYY`, uppercase month. The accepted pattern is
  `^(0[1-9]|[12][0-9]|3[01])-(JAN|FEB|MAR|APR|MAY|JUN|JUL|AUG|SEP|OCT|NOV|DEC)-[0-9]{4}$`;
  an invalid value is a `WriteError`, not a guessed date.

With a blank division the line still ends at column 79 and the date begins at
column 69; the gap is spaces, not a moved date.

### 9.4 Header restoration

- If `include_header_extras` is true, emit each `extras.genbank.header` entry
  in order:
  - `ORGANISM` is emitted with two leading spaces, key padded to column 13,
    then the first value line.
  - Other keys are emitted at column 1, padded to column 13 (or key plus one
    space when longer), then the first value line.
  - Continuation lines are emitted verbatim after right-trimming; a non-empty
    line that does not start with whitespace is prefixed with 12 spaces.
  - Header lines are not wrapped; their stored line breaks are the wrapping.
- If the header has no `DEFINITION` and `description` is not null, synthesize a
  `DEFINITION` entry immediately after `LOCUS`, before any other header entry.
- If the header has a `DEFINITION`, it wins over `description` (round-trip).
- Never synthesize `ACCESSION`, `VERSION`, `KEYWORDS`, `SOURCE`, `ORGANISM`,
  `REFERENCE` or `COMMENT`.
- Ignore header entries whose key is `LOCUS`, `FEATURES` or `ORIGIN`; those are
  structural, and canonical fields are authoritative. (Such entries should not
  exist; this is data hygiene, not a normal path.)
- If `include_header_extras` is false, emit only `LOCUS`, a synthesized
  `DEFINITION` when `description` is present, `FEATURES` and `ORIGIN`.

### 9.5 `FEATURES` layout

Heading line: `FEATURES` followed by spaces to column 21, then
`Location/Qualifiers` at column 22 (the literal line
`FEATURES             Location/Qualifiers`). The heading is emitted even when
the record has zero features.

Feature line:

- Five spaces (columns 1-5).
- Key starting at column 6, left-justified in columns 6-20. If the key is
  longer than 15 characters, emit the key followed by one space and the
  location; the location then starts later than column 22. Keys are never
  truncated.
- Location on the same line, starting at column 22 for keys that fit the
  15-character field.
- The key/location line is never wrapped, even if it exceeds `line_width`.
  Wrapping it would make the reader's line classification ambiguous, and long
  `join(...)` strings are normal in GenBank. This is the deliberate exception
  to the "wrap FEATURES at `line_width`" rule in plan section 5.7; lead
  approved this deliberate deviation on 2026-10-21.

Qualifier lines:

- 21 spaces, then `/key`, then `=`, then the quoted value; the slash starts at
  column 22.
- A `null` value is `/key` (no `=`, no quotes).
- An empty string is `/key=""`.
- Every non-null value is quoted, including numeric-looking values
  (`/codon_start="1"`).
- An embedded double quote is replaced by two single quotes (`''`) and emits
  `WRITE_QUOTE_NORMALIZED` (info). This is the plan section 5.7 and accepted
  model rule. The substitution is one-way: the stored canonical record keeps the
  original `"`, while re-reading the writer output yields `''`. The reader does
  no unescaping.
- Wrapping: compose the first line as 21 spaces + `/key="` + value text +
  closing `"`. If it fits within `line_width` characters, emit one line.
  Otherwise greedily break at single ASCII spaces that fit; the break consumes
  that space, and the reader restores one space when joining. A run with no
  breakable space is emitted whole on its own line, even if it exceeds
  `line_width`; words are never split. Closing quotes appear only on the final
  line. Continuation lines are 21 spaces + text. Trailing spaces are not
  emitted.
- This makes ordinary prose round-trip exactly. Values with leading, trailing
  or repeated spaces are outside the GenBank convention and may not round-trip
  character-for-character; the writer still emits them deterministically.
- `line_width` is a target, not a hard guarantee, for the same reason. A normal
  line is <= 80 columns.

### 9.6 Location serialization

Canonical segments are 0-based half-open; GenBank is 1-based closed.

| Canonical | Text |
| --- | --- |
| `span`, one segment `{s,e}`, `e > s`, no partials | `s+1..e`; if `e == s+1`, `s+1` |
| `span` on a circular record with `e > length` | `s+1..e` (the unwrapped range; at most one wrap) |
| `start_partial` | prefix `<` |
| `end_partial` | prefix `>` |
| single base with both partials | `<s+1..>s+1` |
| zero-length `{n,n}` | `n^n+1` (the site after base `n`) |
| `join` | `join(seg1,seg2,...)` |
| `order` | `order(seg1,seg2,...)` |
| `strand == -1` | `complement(<operator>)` |
| `strand == 0` | plain interval (no `complement`), plus `WRITE_STRAND_LOST` |

Rules:

- Segments are written in array order; `order` and `join` are never sorted or
  merged.
- An unwrapped circular span writes its single 1-based range: `{5899,6100}` on
  a 6000 bp circular record writes `5900..6100`. The writer accepts
  `end > length` only for a circular record with `start < length` and
  `end <= start + length`; a linear record still rejects `end > length`.
- An explicitly split canonical join stays split: `{5899,6000},{0,100}` on a
  6000 bp record writes `join(5900..6000,1..100)`. The writer never merges
  across the origin, never splits a segment, and never rewrites a span into a
  join (or the reverse).
- A zero-length segment is written as `n^n+1`; it requires `n >= 1` and, for a
  linear record, `n < length`; for a circular record, `n <= length`. `{0,0}`
  cannot be written.
- A zero-length segment with partial flags is rejected, not guessed.
- `complement` wraps the whole operator, never individual segments.
- `strand == 0` is written as a plain interval with no `complement` and emits
  `WRITE_STRAND_LOST` (warning); a re-import yields `strand: 1`. This is the
  documented one-way loss for GenBank's missing unstranded spelling.
- A `span` with more than one segment is rejected; if a caller constructed that
  shape, the writer does not silently rewrite it to `join`.

### 9.7 `ORIGIN`

```text
ORIGIN
        1 ATGTTCCATC CGAGGAACCA AGTACAGAGG CTTAATCAAA ACGCTGGGGA CAGCAAAGTG
       61 ATTTCATGTC ACCGACAAAT TAGAACTCCG AGGATCAAAA GTGATTTCAT GAACATTTAA
//
```

- 60 bases per line, six groups of ten, groups separated by one space.
- The line number is the 1-based index of the first base on the line,
  right-justified in 9 columns, followed by one space, so bases start at
  column 11.
- The last line may be shorter; groups keep their 10-base boundaries.
- No trailing spaces. The `//` line follows immediately; the file ends with one
  newline.
- `line_width` does not change the 60-base layout (plan section 5.7 fixes it).
- An empty sequence is impossible for a valid record; the writer rejects it.

### 9.8 `WRITE_*` diagnostics

| Code | Severity | When |
| --- | --- | --- |
| `WRITE_UNSUPPORTED_KIND` | error | `kind` is not `dna`; returned as `WriteError`, no bytes |
| `WRITE_NAME_TRUNCATED` | warning | name longer than 16 characters; `WriteOutcome.diagnostics` |
| `WRITE_QUOTE_NORMALIZED` | info | a qualifier value contained `"` and was rewritten to `''`; `WriteOutcome.diagnostics` |
| `WRITE_MOLECULE_TYPE_DERIVED` | info | `molecule_type` was null, so `ds-DNA` was derived from `kind = dna`; `WriteOutcome.diagnostics` |
| `WRITE_UNSUPPORTED_MOLECULE_TYPE` | error | `molecule_type` is `other`; returned as `WriteError`, no bytes |
| `WRITE_STRAND_LOST` | warning | a location had `strand: 0` and was written without `complement`; `WriteOutcome.diagnostics` |

Writer diagnostics have no source position. They are sorted by the
diagnostic ordering rule (all positionless, then code, then emission order).
The division/date rules and the header omission under
`include_header_extras = false` are explicit options, not loss events, so they
emit no diagnostic. Error-severity `WRITE_*` codes are carried by `WriteError`;
warning and info codes by `WriteOutcome.diagnostics` (section 1.2.1 and
diagnostics.md section 9.4).

### 9.9 Round-trip guarantees

- `import -> write -> import` produces an equal `Record` when
  `provenance` is ignored, for every record that the writer accepts. The
  importer's own output (uppercase IUPAC sequence, canonical locations,
  ordered qualifiers, header in source order) is exactly the writer's input
  domain.
- Both circular forms round-trip. An unwrapped span `{5899,6100}` writes
  `5900..6100` and re-imports as the same one-segment span; a split canonical
  join `{5899,6000},{0,100}` writes `join(5900..6000,1..100)` and re-imports as
  the same two-segment join.
- `write(record)` twice is byte-identical.
- Re-importing the writer's bytes reproduces the same canonical record; the
  second report describes the new bytes (different `source_digest` and
  `source_bytes`) and may carry fewer info diagnostics (for example, quoted
  numeric qualifiers no longer produce `GB_QUALIFIER_UNQUOTED`).
- Lossy cases are explicit: name truncation, quote replacement, dropped
  features on import, `strand: 0` written as forward with `WRITE_STRAND_LOST`
  (re-imports as `strand: 1`), and `molecule_type = other` rejected rather than
  guessed.

### 9.10 M3 implementation notes

- `WriteError` has no code field. `WRITE_UNSUPPORTED_KIND` is
  `WriteError::UnsupportedKind`; `WRITE_UNSUPPORTED_MOLECULE_TYPE` is
  `WriteError::UnsupportedFeature` with the code in its message. Invalid
  records, dates, division values and extras shapes are
  `WriteError::InvalidRecord`.
- The writer calls `Record::validate()` first and refuses a record that is not
  a valid canonical record.
- Qualifier values containing a line feed or carriage return are rejected rather
  than silently flattened. Feature, header and qualifier keys must be single
  tokens; qualifier keys also reject `/`, `=` and `"`. These conditions have
  no dedicated `WRITE_*` code and are `InvalidRecord` paths.
- `line_width` counts bytes. GenBank output is ASCII for every valid record.
  Qualifier wrapping breaks at single ASCII spaces and keeps ordinary prose
  round-trippable; values with unusual runs of spaces remain the documented
  limitation in section 9.5.
- `molecule_type = other` is a successful import (fixture 162) but is
  intentionally not writable: the writer returns
  `WRITE_UNSUPPORTED_MOLECULE_TYPE` instead of guessing a `LOCUS` token.
- Round-trip matrix: every writer-accepted GenBank fixture and golden passes
  `import -> write -> import` with provenance ignored, and repeated writes are
  byte-identical. Fixture 162 is asserted as the explicit rejection case.

## 10. Worked round-trip example

The record is synthetic, 120 bp, circular, and contains one
`complement(join(...))` feature. The digests and byte counts are real and were
recomputed for this note.

### 10.1 Source GenBank record (866 bytes, LF, one final newline)

```text
LOCUS       SYNPLASMID               120 bp    DNA     circular     01-JAN-2000
DEFINITION  synthetic demonstration plasmid.
ACCESSION   SYN0001
VERSION     SYN0001.1
SOURCE      synthetic construct
  ORGANISM  synthetic construct
COMMENT     Synthetic record for the Sequon GenBank adapter design note.
FEATURES             Location/Qualifiers
     source          1..120
                     /organism="synthetic construct"
                     /mol_type="other DNA"
     promoter        1..9
                     /note="synthetic promoter"
     CDS             complement(join(10..30,50..70))
                     /gene="synA"
                     /codon_start=1
                     /transl_table=11
ORIGIN
        1 ATGTTCCATC CGAGGAACCA AGTACAGAGG CTTAATCAAA ACGCTGGGGA CAGCAAAGTG
       61 ATTTCATGTC ACCGACAAAT TAGAACTCCG AGGATCAAAA GTGATTTCAT GAACATTTAA
//
```

The `LOCUS` division field is blank, so no `extras.genbank.division` entry is
produced. `codon_start=1` and `transl_table=11` are unquoted, so the import
report has two `GB_QUALIFIER_UNQUOTED` (info) entries at lines 16 and 17. No
other diagnostics occur: uppercase sequence, LF line ends, lengths match, all
header keys are recognized.

### 10.2 Canonical record

```json
{
  "schema": "sequon/record/1",
  "kind": "dna",
  "molecule_type": "ds-DNA",
  "topology": "circular",
  "name": "SYNPLASMID",
  "description": "synthetic demonstration plasmid.",
  "length": 120,
  "sequence": "ATGTTCCATCCGAGGAACCAAGTACAGAGGCTTAATCAAAACGCTGGGGACAGCAAAGTGATTTCATGTCACCGACAAATTAGAACTCCGAGGATCAAAAGTGATTTCATGAACATTTAA",
  "digests": {
    "algorithm": "sequon-digest/1",
    "sequence": "sha256:5993514390f2cdff65801f9325dfa707472d67d2c50b22b0aa5f2437ee25d65e",
    "canonical": "sha256:46e9105ef2e2d6159fed3634d2c8d9e577c2442afb319674acf01d20679d6c1d"
  },
  "features": [
    {
      "key": "source",
      "so_term": "SO:0002206",
      "name": null,
      "location": {
        "operator": "span",
        "strand": 1,
        "segments": [
          { "start": 0, "end": 120, "start_partial": false, "end_partial": false }
        ]
      },
      "qualifiers": [
        { "key": "organism", "value": "synthetic construct" },
        { "key": "mol_type", "value": "other DNA" }
      ]
    },
    {
      "key": "promoter",
      "so_term": "SO:0000167",
      "name": null,
      "location": {
        "operator": "span",
        "strand": 1,
        "segments": [
          { "start": 0, "end": 9, "start_partial": false, "end_partial": false }
        ]
      },
      "qualifiers": [
        { "key": "note", "value": "synthetic promoter" }
      ]
    },
    {
      "key": "CDS",
      "so_term": "SO:0000316",
      "name": "synA",
      "location": {
        "operator": "join",
        "strand": -1,
        "segments": [
          { "start": 9, "end": 30, "start_partial": false, "end_partial": false },
          { "start": 49, "end": 70, "start_partial": false, "end_partial": false }
        ]
      },
      "qualifiers": [
        { "key": "gene", "value": "synA" },
        { "key": "codon_start", "value": "1" },
        { "key": "transl_table", "value": "11" }
      ]
    }
  ],
  "provenance": {
    "producer": { "kind": "parser", "name": "sequon-io", "version": "0.1.0" },
    "source_format": "genbank",
    "source_digest": "sha256:6e00a35abfbe90602fc57a0dec51dce68127d3bc6a8311428ca8c6badd81070b",
    "source_bytes": 866,
    "export_version": null
  },
  "extras": {
    "genbank": {
      "header": [
        { "key": "DEFINITION", "value": "synthetic demonstration plasmid." },
        { "key": "ACCESSION", "value": "SYN0001" },
        { "key": "VERSION", "value": "SYN0001.1" },
        { "key": "SOURCE", "value": "synthetic construct" },
        { "key": "ORGANISM", "value": "synthetic construct" },
        { "key": "COMMENT", "value": "Synthetic record for the Sequon GenBank adapter design note." }
      ]
    }
  }
}
```

The block is indented for readability. The canonical serializer emits one
member per line (canonical-json-profile.md section 7); whitespace is not
semantic here.

### 10.3 Mapping walkthrough

| Source element | Canonical value |
| --- | --- |
| `LOCUS SYNPLASMID` | `name: "SYNPLASMID"` |
| `LOCUS 120` | `length: 120`, checked against `ORIGIN` (equal) |
| `LOCUS DNA` | `kind: "dna"`, `molecule_type: "ds-DNA"` |
| `LOCUS circular` | `topology: "circular"` |
| blank division | no `extras.genbank.division` |
| `DEFINITION` | `description` and header entry 0 |
| `ACCESSION`, `VERSION`, `SOURCE`, `ORGANISM`, `COMMENT` | header entries 1-5, in source order |
| `source 1..120` | span `{0,120}`, `SO:0002206`, name `null` |
| `promoter 1..9` | span `{0,9}`, `SO:0000167`, name `null` |
| `CDS complement(join(10..30,50..70))` | `join`, `strand: -1`, `{9,30}`, `{49,70}`; `SO:0000316`; name `synA` |
| `ORIGIN` lines | 120 uppercase bases, grouped as six 10-base groups per line |

`complement` changes only `strand`; the segment order `10..30` then `50..70`
is preserved. The first segment starts at 0-based 9, the second at 49.

### 10.4 Writer output (870 bytes)

```text
LOCUS       SYNPLASMID               120 bp    DNA     circular     01-JAN-2000
DEFINITION  synthetic demonstration plasmid.
ACCESSION   SYN0001
VERSION     SYN0001.1
SOURCE      synthetic construct
  ORGANISM  synthetic construct
COMMENT     Synthetic record for the Sequon GenBank adapter design note.
FEATURES             Location/Qualifiers
     source          1..120
                     /organism="synthetic construct"
                     /mol_type="other DNA"
     promoter        1..9
                     /note="synthetic promoter"
     CDS             complement(join(10..30,50..70))
                     /gene="synA"
                     /codon_start="1"
                     /transl_table="11"
ORIGIN
        1 ATGTTCCATC CGAGGAACCA AGTACAGAGG CTTAATCAAA ACGCTGGGGA CAGCAAAGTG
       61 ATTTCATGTC ACCGACAAAT TAGAACTCCG AGGATCAAAA GTGATTTCAT GAACATTTAA
//
```

The writer emits no diagnostics (`WriteOutcome.diagnostics` is empty): the name
fits, `molecule_type` is present, and no qualifier contains a double quote. The
only byte changes are the four quote characters around `1` and `11`. Re-import
of these bytes produces the same canonical record (same sequence, digests,
features, qualifiers, header); `source_digest` and `source_bytes` describe the
new 870 bytes, and the two `GB_QUALIFIER_UNQUOTED` info events disappear because
the writer quoted the values. This is the expected `import -> write -> import`
semantics: record equality, not report equality.

## 11. Fixture inventory

The 20 rows of plan section 7.2 are authoritative (the M0 card's "21" was a
typo). Fixtures are conformance-owned and are not created by this note. The
"proving code" is the diagnostic the test must assert; a successful fixture
with no code asserts the record shape.

| Fixture | What it covers | Expected outcome | Proving code / assertion |
| --- | --- | --- | --- |
| `001-minimal-cds.gb` | LOCUS, ORIGIN, one CDS | success | one record, one CDS span, length matches |
| `010-circular-plasmid.gb` | circular record, several features, circular digest | success | topology circular, canonical digest of a rotation/revcomp case, features in order |
| `020-join-complement.gb` | `complement(join(...))` | success | `strand: -1`, segment order preserved |
| `030-order.gb` | `order(...)`, repeated coordinates | success | `operator: "order"`, segments not sorted |
| `040-near-origin.gb` | one segment crossing the origin | success | one unwrapped segment `{5899,6100}`, `operator: "span"`, `end > length` on the circular record |
| `050-partial.gb` | `<` and `>` boundaries | success | `GB_LOCATION_PARTIAL_BOUNDARY` (info); flags set |
| `060-duplicate-qualifiers.gb` | repeated `/db_xref`, multi-line `/note` | success | duplicates and order preserved; note joined by one space |
| `070-unknown-key.gb` | unknown feature key | success | `GB_UNKNOWN_FEATURE_KEY` (info); key kept; `so_term: null` |
| `080-multi-record.gb` | two records | `All`: both; `First`: first; `Reject`: error | `GB_MULTIPLE_RECORDS_NOT_ALLOWED` for `Reject` |
| `090-locus-length-mismatch.gb` | LOCUS length differs from ORIGIN | success | `GB_LOCUS_LENGTH_MISMATCH` (warning); actual length wins |
| `100-crlf.gb` | CRLF, no trailing newline | success | `GB_CRLF_NORMALIZED` (info); same record as LF |
| `110-no-features.gb` | zero features | success | `features: []` |
| `120-unterminated.gb` | missing `//` | error | `GB_RECORD_UNTERMINATED` |
| `130-rna.gb` | `mRNA` record | error | `GB_UNSUPPORTED_MOLECULE` |
| `140-bad-location.gb` | unmatched parenthesis | error | `GB_LOCATION_MALFORMED` with line and column |
| `150-large-1mb.gb` | 1 Mb sequence | success | within limits, memory linear in input |
| `160-missing-name.gb` | no LOCUS name, no hint | error | `RECORD_REQUIRED_FIELD_MISSING { field: "name" }` |
| `161-missing-topology.gb` | no topology, no hint | error | `GB_TOPOLOGY_MISSING` |
| `162-unknown-molecule.gb` | unknown DNA subtype | success | `MOLECULE_TYPE_UNKNOWN` (warning); `molecule_type: "other"` |
| `163-name-hint.gb` | no LOCUS name, `name_hint` supplied | success | `name` from the hint |

`162` needs a token that is unambiguously DNA but not `DNA`/`ds-DNA`/`ss-DNA`;
the natural choice is `ms-DNA` (mixed-stranded DNA), which the reader maps to
`other` under section 3.6. The fixture owner picks the final token.

### 11.1 Codes without a matrix row

Plan section 7.6 requires every code to have a fixture **or** a unit or
integration test. These GenBank codes are covered by focused tests per
`testdata/README.md` section 5.3, not by a new fixture row: `GB_EMPTY_INPUT`,
`GB_CONTENT_BEFORE_LOCUS`, `GB_MISSING_LOCUS`, `GB_MALFORMED_LOCUS`,
`GB_MISSING_ORIGIN`, `GB_SEQUENCE_EMPTY`, `GB_ORIGIN_INVALID_CHAR`,
`GB_RNA_BASE_IN_DNA`, `GB_LOCATION_UNSUPPORTED`, `GB_COORDINATE_OUT_OF_RANGE`,
`GB_QUALIFIER_UNQUOTED`, `GB_LOCATION_BETWEEN_POSITIONS`,
`GB_FEATURE_COLUMN_MISALIGNED`, `GB_SEQUENCE_NORMALIZED`,
`GB_HEADER_FIELD_UNKNOWN`, `GB_ENCODING_INVALID`, `GB_FEATURES_MALFORMED`,
`LIMIT_EXCEEDED`, `RECORD_FIELD_INVALID`, plus the six writer codes (including
`WRITE_UNSUPPORTED_MOLECULE_TYPE` and `WRITE_STRAND_LOST`). No fixture is
invented here.

## 12. Open format facts to verify with fixtures

Only format facts, not design decisions. Each is expected to be pinned by the
fixture suite or a real corpus before M1/M3 is called done.

| # | Fact | Why it matters | Proposed proof |
| --- | --- | --- | --- |
| F1 | Qualifier continuation semantics: line break -> one space, and whether real files ever break mid-word | Reader joins with one space; a mid-word break would corrupt a value | fixture with a multi-line `/note`; corpus check |
| F2 | Whether third-party GenBank files wrap the feature location line | v1 appends a pre-qualifier continuation to the location; writer never wraps | corpus file with a long `join(...)` |
| F3 | Real `LOCUS` molecule token vocabulary (`ms-DNA`, `other DNA`, ...) | Decides `other` + `MOLECULE_TYPE_UNKNOWN`; the writer rejects `other` | fixture 162 plus corpus |
| F4 | `LOCUS` column drift (long name, missing name, nonstandard spacing) | Token fallback and fixed topology columns must agree | unit cases + fixtures 160/161/163 |
| F5 | Unknown topology token versus blank topology | Blank -> hint/`GB_TOPOLOGY_MISSING`; non-blank unknown -> `RECORD_FIELD_INVALID` | unit case with `triangle` in columns 56-63 |
| F6 | `ORIGIN` numbering widths and lines without numbers | The stripper must not eat bases or accept digits as bases | unit cases |
| F7 | `//` with trailing spaces, and trailing blank lines | Terminator recognition and `GB_RECORD_UNTERMINATED` | unit cases; fixture 100 |
| F8 | Header continuation indentation for `REFERENCE` subfields and `ORGANISM` taxonomy | Raw-continuation storage and writer restoration | real-record fixture; build from a committed synthetic header |
| F9 | Between-site at the origin (`L^L+1`) and at a linear last base | Zero-length segment bounds | unit cases |
| F10 | CRLF versus lone CR and UTF-8 BOM | Normalization and `GB_CONTENT_BEFORE_LOCUS` | unit cases |
| F11 | Valid UTF-8 beyond ASCII in headers and qualifiers | v1 preserves it; invalid UTF-8 is `GB_ENCODING_INVALID` | corpus check |
| F12 | Division token position in older `LOCUS` layouts | Token fallback versus fixed columns 65-67 | corpus check |

## 13. Cross-check performed

Every row of plan section 5 was checked against this note, and against the
accepted contract documents.

| Plan section | Check |
| --- | --- |
| 5.1 record boundaries | Covered in section 2: `LOCUS` start, `//` terminator, content before `LOCUS`, CRLF + info, empty input, extension not a discriminator. |
| 5.2 header | Covered in section 3: all mapped fields, `extras.genbank.header`, `description`, unknown keys + info, D16 failure codes, molecule subtype table, v1 unsupported molecules. |
| 5.3 FEATURES | Covered in section 4: priority, column drift + info, quoted/unquoted/bare qualifiers, duplicate keys, D11 skip-vs-abort with `dropped[]`. |
| 5.4 location | Covered in section 5: every grammar row, circular coordinate normalization (unwrapped span versus split join), linear versus circular, malformed/unsupported/limit split, depth/segment limits. |
| 5.5 feature semantics | Covered in section 7: key verbatim, static SO mapping, no qualifier rewriting, unknown key -> `null`. |
| 5.6 ORIGIN | Covered in section 6: stripping, uppercase + info, IUPAC set, `U`, invalid char with line, empty sequence, max length. |
| 5.7 writer | Covered in section 9: kind restriction, LOCUS layout, 80-column wrapping, 21-space continuation, quoting + warning, location re-serialization, ORIGIN 60/line, trailing `//`, header restoration, no fabricated `ACCESSION`, byte determinism, `WRITE_*`. |
| 5.8 multi-record | Covered in section 8: three policies, per-record diagnostics, `max_records`. |
| 5.9 unsupported molecules | Covered in section 3.6. |

Consistency checks and arithmetic:

- D1-D16: D2 0-based half-open with unwrapped circular spans (revised
  2026-10-23) and explicit split joins; D3 ordered duplicate
  qualifiers; D6/D7 diagnostics only in the report; D8/D9 namespace-isolated
  header extras; D10 three policies; D11 skip/abort split; D12 typed limits;
  D15/D16 kind and required fields. No new model field is invented.
- Code table `2026-10-21/1`: every code used here exists in
  [diagnostics.md](diagnostics.md) with the stated severity and position;
  none is invented.
- Schema: `features[].key/so_term/name/location/qualifiers`, ordered
  qualifiers, `molecule_type`, `location.operator/strand/segments`, and the
  `dropped[]` `feature` fields match
  `schemas/sequon-record-1.schema.json` and
  `schemas/import-report-1.schema.json`.
- JSON profile: header values are single strings with `\n` escapes; extras keys
  are sorted by the serializer; features, qualifiers, segments and header stay
  in source order.
- Arithmetic: `LOCUS` layout is 79 columns and the date starts at column 69;
  an `ORIGIN` line is 9 + 1 + 60 + 5 = 75 columns; `5900..6100` on 6000 bp
  gives one unwrapped segment `{5899,6100}` (201 bases; 101 + 100 resolved at
  the origin), and an explicit `join(5900..6000,1..100)` gives
  `{5899,6000},{0,100}`; the example sequence is 120 bases, its two digests
  were recomputed, and the input/output byte counts are 866/870.
- SO identifiers: checked against the Sequence Ontology OBO release
  `2026-08-07`; 44 rows, all with an explicit basis.
- Fixture count: 20, matching plan section 7.2 after the lead's correction.

Findings resolved by lead decisions on 2026-10-21:

1. The accepted model's cross-origin example was off by one (`{5900,6000}` /
   `5901..6000`). Corrected to `{5899,6000}` / `5900..6000`; the 2026-10-23
   revision then stopped splitting the span (section 14.1).
2. Malformed location means an error for that record; unsupported and linear
   out-of-range mean skip. Coordinate value above `max_sequence_len` is
   `LimitExceeded`.
3. Feature `name` comes from the first `/gene`, else `null`.
4. `WriteOutcome` was approved for writer diagnostics.
5. `LOCUS` division is preserved as `extras.genbank.division`; the source date
   is intentionally not preserved.
6. A1-A12 were settled: new codes `GB_ENCODING_INVALID`,
   `GB_FEATURES_MALFORMED`, `WRITE_UNSUPPORTED_MOLECULE_TYPE` and
   `WRITE_STRAND_LOST`; quote, strand, wrapping, slash-precedence and
   normalization wording follow the lead decisions.

All A-items are resolved; section 14 records the outcome.

## 14. Resolved M0 decisions

All items raised while drafting this note were decided by the lead on
2026-10-21 and are encoded in the sections above. No open design item remains
for M0.

| Was | Resolution |
| --- | --- |
| A1 `molecule_type = "other"` | Writer rejects with `WRITE_UNSUPPORTED_MOLECULE_TYPE` (error in `WriteError`); no token guessing and no raw-token field in v1. |
| A2 `strand = 0` | Writer emits a plain interval with `WRITE_STRAND_LOST` (warning); a re-import yields `strand: 1`. |
| A3 invalid UTF-8 | `GB_ENCODING_INVALID` (error, line). v1 requires valid UTF-8 and has no lossy or latin-1 path. |
| A4 `First` indexing | `record_index` is the 0-based parse-order ordinal, not an array offset; non-returned records may have no `records[]` entry. |
| A5 malformed location | `GB_LOCATION_MALFORMED` is malformed syntax only; limits are `LimitExceeded`. diagnostics.md is corrected. |
| A6 writer carriers | Error `WRITE_*` codes travel in `WriteError`; warning and info codes in `WriteOutcome.diagnostics`. |
| A7 quote rule | Embedded `"` becomes `''` with `WRITE_QUOTE_NORMALIZED`; the substitution is one-way and the stored record keeps the original. |
| A8 structural FEATURES | `GB_FEATURES_MALFORMED` (error, line) replaces the `GB_LOCATION_MALFORMED` fallback. |
| A9 FEATURES wrapping | Qualifier values wrap; key/location lines never wrap and may exceed `line_width`. Third-party wrapped locations remain F2. |
| A10 profile example | Contract is fixing the illustrative example; the old example is not authoritative. |
| A11 slash precedence | `/` is tested before the column<21 rule; approved. |
| A12 normalization wording | `GB_SEQUENCE_NORMALIZED` means lowercase bases were uppercased; diagnostics.md is aligned. |

### 14.1 Revision 2026-10-23: circular coordinates

The lead revised D2 for circular records. The canonical payload keeps the
source form instead of normalizing it to a split join:

- A single GenBank range crossing the origin, such as `5900..6100` on a 6000 bp
  circular record, becomes one `span` segment `{5899,6100}` with `end > length`;
  it is not split and `operator` is not rewritten.
- An explicit source `join(5900..6000,1..100)` stays two `join` segments
  `{5899,6000},{0,100}`; the operator is preserved and nothing is merged.
- The writer reverses each form faithfully: an unwrapped span writes
  `5900..6100`; a split join keeps `join(5900..6000,1..100)`.
- Linear records are unchanged: `0 <= start <= end <= length` and the writer
  still rejects `end > length`. Circular validation is
  `0 <= start < length` and `end <= start + length`, with zero-length sites at
  `0 <= start = end <= length`.
- `crates/sequon-model` owns the topology-aware validation and the
  `Segment::is_wrapped`, `Segment::resolved`, `Location::is_wrapped` and
  `Location::resolved_segments` accessors; the adapter does not duplicate them.

## 15. References

- `docs/design/canonical-model.md` (accepted) - record semantics, D1-D16,
  `extras.genbank.header`, `extras.genbank.division`, required fields.
- `docs/design/diagnostics.md` (accepted) - code table `2026-10-21/1`,
  severities, positions, ordering, `dropped[]` vocabulary.
- `docs/design/canonical-json-profile.md` (accepted) - field order, string
  escaping, extras ordering, determinism.
- `schemas/sequon-record-1.schema.json`,
  `schemas/import-report-1.schema.json` - machine-readable shape.
- `testdata/README.md` section 5 - fixture manifest and error-path coverage.
- Development plan sections 3.4, 4.9, 5, 7.2, 7.4, 7.6, 9.2 (task record, not
  a repository file).
