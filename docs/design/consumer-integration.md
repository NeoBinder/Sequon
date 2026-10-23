# Consumer integration: pinning, payload ownership and report handling

Status: **accepted**, M5 (2026-10-21); revised 2026-10-22 for the platform
projection P1 and the version pin, compatibility window and schema delivery
decisions; revised 2026-10-23 for circular cross-origin coordinates. This note
records consumer duties; it does not
add fields or requirements to `sequon/record/1` or `sequon/import-report/1`,
whose semantics stay owned by [canonical-model.md](canonical-model.md). It is
written for a platform executor such as `<NeoAPI-rs>` and references the
compiled example [`examples/platform-import.rs`](../../crates/sequon-io/examples/platform-import.rs).

Audience: engineers wiring Sequon into a service that accepts user sequence
files. Sequon is a library; it has no routes, jobs, database or UI. The platform
owns all of that and must not recreate the payload contract.

## 1. Ownership boundary

| Concern | Owner | Rule |
| --- | --- | --- |
| Canonical payload `sequon/record/1` | Sequon | Field semantics, schema and byte profile. The platform consumes it; it never adds, removes or redefines a field. |
| Import report `sequon/import-report/1` | Sequon | Event document produced by the adapter. The platform stores and forwards it; it never writes diagnostics back into a record. |
| Diagnostic codes, severities, positions, `codes_version` | Sequon | Append-only vocabulary in [diagnostics.md](diagnostics.md). The platform keeps unknown codes verbatim and never invents one. |
| Digests (`sequon-digest/1`) | Sequon | The canonical digest is the cross-version identity. The platform verifies it and never recomputes it under a different algorithm id. |
| Routes, auth, request/response DTOs, job envelope, 202/409 | Platform | Not part of this repository. The payload is embedded unchanged or referenced by object key; transport metadata lives beside it. |
| Persistence DDL, table/column names, indexes | Platform | The platform may project fields under its own names; canonical segments remain authoritative. No library-level projection is assumed (see section 6). |
| Original source bytes and retention | Platform | Store the exact bytes (section 5). Sequon records their digest, not their location. |

The executor is a thin wrapper. It must not re-parse the source with its own
logic, re-normalize coordinates, re-derive digests, sort `order` locations, or
interpret `extras`. If it needs a derived value, it derives it from the
canonical record, not from the source text.

Dependency shape for an executor:

```rust
use sequon_io::api::{ImportOptions, ImportOutcome};
use sequon_model::Record;

fn import(source: &[u8], options: &ImportOptions) -> Result<ImportOutcome, sequon_io::ImportError> {
    sequon_io::import(source, options)
}

fn acceptable(record: &Record) -> Result<(), String> {
    record.validate().map_err(|error| error.to_string())?;
    sequon_algo::verify_digests(record).map_err(|error| error.to_string())
}
```

The executor depends on `sequon-io` for adapters, `sequon-model` for
`Record::validate()` and `sequon-algo` for `verify_digests()`. The split is
deliberate: the model does not hash, so a consumer must call both checks.
`sequon-io` defaults to the `genbank` feature; the `snapgene` feature is
optional. Build the executor with exactly the formats the route advertises.

## 2. Version pins

A payload and its report are only interpretable with the contract versions that
produced them. Persist these pins with every import:

| Pin | Value (version 1) | Where it comes from |
| --- | --- | --- |
| record schema id | `sequon/record/1` | `record.schema` (`sequon_model::SCHEMA_ID`) |
| record schema `$id` | `urn:sequon:schema:record:1` | `sequon_model::RECORD_SCHEMA_URN` (embedded `$id`) |
| record schema bytes SHA-256 | hash of the exact schema bytes embedded in the library | SHA-256 of `sequon_model::RECORD_SCHEMA` |
| report schema id | `sequon/import-report/1` | `report.schema` (`sequon_io::REPORT_SCHEMA_ID`) |
| report schema `$id` | `urn:sequon:schema:import-report:1` | `sequon_io::REPORT_SCHEMA_URN` (embedded `$id`) |
| report schema bytes SHA-256 | hash of the exact schema bytes embedded in the library | SHA-256 of `sequon_io::REPORT_SCHEMA` |
| producer | `parser sequon-io <crate version>` | `record.provenance.producer`, `report.parser` |
| source digest | `sha256:...` of the exact input bytes | `report.source_digest`, `record.provenance.source_digest` |
| source bytes | integer length of the input | `report.source_bytes`, `record.provenance.source_bytes` |
| SnapGene export version | integer, or `null` for GenBank | `report.export_version`, `record.provenance.export_version` |
| codes version | `2026-10-21/1` | `report.codes_version` |
| digest algorithm | `sequon-digest/1` | `record.digests.algorithm` |

Rules:

- **Do not copy the schema body.** Pin the `$id`, the SHA-256 of the schema
  bytes and the producer version. A cross-repository check uses the byte hash as
  its credential; contract tests stay in Sequon.
- **Read the schema bytes from the library.** The exact bytes are embedded at
  compile time as `sequon_model::RECORD_SCHEMA` and
  `sequon_io::REPORT_SCHEMA`, alongside the `$id` constants
  `sequon_model::RECORD_SCHEMA_URN` and `sequon_io::REPORT_SCHEMA_URN`. The
  platform reads those bytes at run time and computes the SHA-256 for pinning;
  no release asset and no `schemas/PINS.json` is required. A published copy
  under `schemas/` remains available to non-Rust consumers.
- **Hash the bytes exactly as embedded** (no JSON re-serialization, no
  whitespace normalization). The bytes are frozen per crate release, so a schema
  change requires a crate release and one producer version maps to one schema
  hash.
- **Store the pins with every import.** The pins are the schema `$id`, the
  schema bytes SHA-256, the producer version, `codes_version`, the digest
  algorithm and the source digest. A mismatch never blocks startup; the platform
  records it and grades it by the compatibility window (section 2.1).
- The producer version is self-reported by the library. The executor image
  pins the crate version, so a mismatch against the deployment manifest is a
  packaging defect to report, not a reason to refuse service.
- `record.schema` and `report.schema` are the payload-level strings; the `$id`
  values are schema-level identifiers. Both are pinned; the mapping is fixed
  for version 1.

### 2.1 What changes between Sequon versions

- **Byte identity is promised only within one crate version** (D5). A payload
  re-serialized by a newer Sequon may differ in bytes without any semantic
  change. Never compare canonical JSON bytes across versions.
- **Canonical digests are the permanent identity.** They must not change across
  parser versions. A changed canonical digest after an upgrade is a contract
  violation to escalate, not a migration to accept.
- **Within schema version 1, an optional field may be added.** Sequon's Rust
  types deny unknown fields, so a reader older than the producer can reject a
  newer payload. Version 1 readers are strict, and an additive field changes the
  embedded schema bytes and their SHA-256; that hash, the `$id` and the producer
  version are the skew signals a consumer records.
- **Compatibility window (semver).** A reader R accepts a payload produced by
  producer P if and only if `P.major == R.major` and (`P.minor < R.minor`, or
  `P.minor == R.minor` and `P.patch <= R.patch + 1`). A producer on a higher
  minor, or on the same minor with `P.patch > R.patch + 1`, requires a reader
  upgrade first. A major bump may break compatibility and requires a migration
  with both versions coexisting while stored data is upgraded.
- **A pin mismatch never blocks startup.** When the stored producer version is
  newer than the running reader but inside the window, startup proceeds and the
  data is upgraded during loading: re-import the retained original bytes with
  the newer reader, or migrate the stored payload. Outside the window, upgrade
  the reader before loading the data. Sequon reports the schema ids,
  `codes_version` and crate version so the platform can make that comparison.
- `codes_version` is independent of the schema and crate version. Adding a
  diagnostic code bumps it; a schema addition does not.
- `digests.algorithm` is independent of everything else. Never recompute a
  digest under a different algorithm id; if the id is unknown, store the value,
  flag it and do not claim verification.

## 3. Consuming `ImportOutcome` and `ImportReport`

`sequon_io::import` returns `ImportOutcome { records, report }` on success and
a typed `ImportError` on failure. A failure produces no report and no record;
partial multi-record success produces records plus a report that may contain an
error-severity diagnostic.

### 3.1 Records

- Each entry of `outcome.records` is the canonical payload. Persist it as
  canonical JSON (bytes produced by `sequon_io::json::to_canonical_json_bytes`)
  or project fields under platform names; both are views of the same record.
- A record describes one molecule at one point in time and is immutable by
  contract. Editing creates a new record; diagnostics from the source file are
  not inherited.
- Use `digests.canonical` as the molecule identity across rotations and
  strands, and `digests.sequence` as the direction-sensitive identity of the
  recorded entry. A platform column named `seq_digest` stores
  `digests.canonical` (section 6.4); `digests.sequence` may be stored separately
  but is not the identity.

### 3.2 Report summary and diagnostics

| Field | Consumer use |
| --- | --- |
| `report.records[]` | Lightweight listing/status: `index`, `name`, `kind`, `length`, digests. Lists **returned** records only. |
| `report.diagnostics[]` | Severity counts, filtering, alerts and an audit trail. Never parse `message` for control flow. |
| `report.dropped[]` | Loss inventory: `what`, `reason`, optional fingerprint fields. Store; it is the proof that nothing was lost silently. |
| `report.preserved[]` | Raw skipped blocks as base64 with `what`, `encoding`, optional offset/length/digest. Store or offload; do not decode and reinterpret. |
| `report.source_format`, `source_digest`, `source_bytes`, `export_version` | Provenance of the exact source; used to find and verify the original bytes. |
| `report.parser` | Producer kind/name/version; same value as `record.provenance.producer` and part of the pin. |
| `report.codes_version` | Which diagnostic table produced the report; store with the diagnostic rows. |

Diagnostic handling rules from [diagnostics.md](diagnostics.md):

- Keep `code`, `severity`, `message`, `position` and `record_index` unchanged.
- An **unknown code must not reject the report**. Map it to an `unknown` class
  for filtering/display, keep the raw string, and surface it at least once (a
  count or aggregate log line is enough).
- **Severity is authoritative.** Never derive severity from the code spelling
  and never upgrade a warning to an error.
- `position` refers to the source (GenBank line/column, SnapGene byte offset
  and optional packet fields), never to canonical coordinates.
- `record_index` is the 0-based parse-order ordinal, not necessarily an index
  into `records[]`. Under `MultiplePolicy::First` the remaining records are
  still parsed for diagnostics, so their `record_index` has no returned record.
  Join a diagnostic to a payload only when that index is present in
  `outcome.records`.
- Error-severity entries in a report occur only when at least one usable record
  was returned (multi-record partial success). Do not persist the record named
  by such an entry.
- `dropped` and `preserved` are not diagnostics and are mutually exclusive for
  the same range: kept bytes go to `preserved`, fingerprinted bytes go to
  `dropped`.

### 3.3 Report consistency checks

The library guarantees the shape; the executor should still check that the
event document and the payload agree before either is persisted. The example
performs these checks; a platform should do the same or stronger:

- `report.schema` is `sequon/import-report/1` and every `record.schema` is
  `sequon/record/1`;
- `report.source_digest` equals the SHA-256 of the bytes actually read and
  `report.source_bytes` equals their length;
- `report.records.len()` equals `outcome.records.len()` under
  `MultiplePolicy::All` (the default); under `First` the report lists only
  returned records;
- for each returned record, the report summary's `name`, `kind`, `length`,
  `sequence_digest` and `canonical_digest` equal the payload values;
- each record's `provenance.source_digest`, `source_bytes`, `source_format`,
  `export_version` and `producer` equal the report's;
- `report.source_format` agrees with what `sequon_io::detect` returned.

A mismatch is a library defect to report upstream, not a user input error.

### 3.4 Typed failures

`ImportError` carries a stable `code()`, a message and an optional source
position. Variants: `LimitExceeded`, `Malformed`, `Unsupported`,
`MissingRequired`, `FormatUnknown`. Map them to transport errors in the
platform's own vocabulary; do not invent a second payload error format.

- `LimitExceeded` exposes the limit name, configured value and observed value.
  Choose a configuration where the platform upload limit is stricter or equal
  to `Limits::max_input_bytes` (default 256 MiB); a limit hit must not become a
  partial import.
- `MissingRequired` names the field (`name`, `kind` or `topology`). If the
  platform offers caller hints, supply them deliberately; a hint is used only
  when the format provides no value and never overrides parsed data.
- `FormatUnknown` also covers an adapter that is not compiled into the build
  (for example SnapGene input with `snapgene` disabled). Ensure the executor
  build advertises only formats whose adapters are enabled.

## 4. Validation duties before persistence

Call both checks for every record, in this order, after import and after any
later parse of stored payload bytes:

1. `record.validate()` - cross-field invariants: schema id, non-empty name and
   producer, `length` equals sequence length, non-empty uppercase IUPAC
   sequence, segment bounds and operator/segment arity, strand range, qualifier
   keys, digest syntax.
2. `sequon_algo::verify_digests(record)` - recomputes `digests.sequence` and
   `digests.canonical` from the sequence and topology and compares them with the
   stored values.

JSON Schema is not a substitute. It validates structure, enums and patterns but
cannot compare fields with each other, so it cannot check `length` against
`sequence`, segment bounds against `length`, or digest values. The schemas
state this in their descriptions; `Record::validate()` owns those invariants.

Do not persist a record that fails either check. Treat the failure as a producer
defect (library bug or corrupted stored payload), not as user input to repair.
When re-reading a stored payload, parse with
`sequon_io::json::parse_canonical_json` before validating; it rejects unknown
fields and non-canonical number syntax.

For non-Rust consumers there is currently no language-neutral validator
(section 8). Cross-field checks and digest recomputation require the Rust
executor, so do not bypass it for validation.

## 5. Store the original bytes

`provenance.source_digest` and `report.source_digest` are the SHA-256 of the
exact input bytes. Store those bytes (or an immutable object reference to them)
with every import:

- **Re-parsing.** A newer Sequon version may fix a parser defect or add format
  support; re-import needs the original bytes. The canonical payload alone may
  not reproduce diagnostics, dropped/preserved blocks or export version.
- **Verification.** Before reusing stored bytes, recompute their SHA-256 and
  compare with the pin. Content-addressed storage (key = `source_digest`) gives
  deduplication and detects corruption.
- **Audit.** Diagnostic positions (GenBank lines, SnapGene offsets) refer to
  those bytes and remain explicable only while they are retained.
- **Idempotency.** The same bytes, options and Sequon version produce the same
  records and digests. `source_digest` is a natural idempotency component; the
  platform decides the rest of the key and the 202/409 behavior.

If bytes cannot be retained (retention policy, user deletion), keep the digest
and length anyway and mark the import as not re-parsable. Never store only the
canonical payload while claiming the source is recoverable.

Treat source bytes and canonical payload as user data: access control,
encryption and retention are platform decisions. Diagnostic messages are
contractually free of sequence content and machine paths, but payloads are not;
do not log record content.

## 6. Platform coordinate projection P1

The canonical payload is unchanged in shape and authoritative: segments are
0-based half-open, and on a circular record a single range that crosses the
origin is already stored unwrapped as one segment with `end > length` (one wrap
at most), while an explicitly split source such as a GenBank `join` stays split.
A linear record never carries `end > length`; that stays invalid. P1 is a
platform-projection decision: the platform may store a coarse unwrapped interval
for a circular record and resolve the wrap on access, but the row is an index
over the canonical segments, not a second representation. A platform coarse row
for a canonical single circular range can copy the already-unwrapped segment;
the origin-ordered split pieces come from `Location::resolved_segments(length)`.
The library emits no platform row shape and assumes no projection.

### 6.1 Coarse single-row index (P1.1, P1.2, P1.3)

If the platform keeps one row per feature, the row is an index over the
canonical segments, never a second representation. Keep `operator`, `strand`
and the full segment list in the canonical payload (or a JSON column), and
re-check the segments after a coarse index hit.

**Circular by type; wrap resolved on access (P1.1).** For a record of length
`L`, the row stores an interval `(start_pos, end_pos)` that may run past the
origin when the record is circular. A stored interval `(s, e)` with
`0 <= s < L` and `e <= s + L` resolves as:

```text
resolve(s, e, L):
    if e <= L:   [s, e)
    else:        [s, L) + [0, e - L)
```

- A canonical single range that crosses the origin is already unwrapped, so the
  platform copies the segment: for a 6000 bp circular record whose canonical
  location is one `span` segment `{5899, 6100}` (GenBank `5900..6100`), the row
  is `(5899, 6100)`, which resolves back to `[5899,6000) + [0,100)`.
- An explicitly split join (for example GenBank `join(5900..6000,1..100)` with
  canonical segments `[{5899,6000},{0,100}]`) may be stored as the unwrapped
  `(first_segment.start, L + last_segment.end)`, which is `(5899, 6100)` when
  the segments form a contiguous origin-crossing chain, or as the
  `min(segment.start) .. max(segment.end)` superset with a segment post-filter.
  Both are coarse indexes over the same payload; the segment-level rows hold the
  exact resolved pieces.
- A query resolves the stored interval with the same rule before a half-open
  overlap test, so a wrapped row answers correctly without being stored split.
- Linear records never wrap: the row satisfies
  `0 <= start_pos <= end_pos <= length`, and `end > length` stays invalid. On a
  circular record `end_pos > length` is a normal coarse interval, not a
  validation failure.
- Do not write the projected interval back into the canonical payload, and do
  not reconstruct a single joined interval as truth.

**Zero-length sites carry a discriminator (P1.2).** A site row stores the triple
`(start_pos, end_pos, site_kind)`:

| `site_kind` | Meaning | Constraint |
| --- | --- | --- |
| `NULL` | span | `start_pos < end_pos` |
| `0` | on-position (a position on the sequence) | `start_pos == end_pos` |
| `1` | between-site (a site in the gap between two adjacent bases) | `end_pos == start_pos + 1` |

The loader validates `site_kind = 1 => end_pos == start_pos + 1` and
`site_kind = 0 => start_pos == end_pos`. A canonical zero-length segment `{s,s}`
is a GenBank `n^m` between-site at boundary `s` and maps to `(s-1, s, 1)` for
`s >= 1`; `s = 0` does not arise from `n^m` in version 1. A strict half-open
overlap predicate must not match site rows, and a site row must not be treated
as an empty feature: index and render it.

**`order` is deferred (P1.3).** The operator `order` is preserved verbatim;
segment order is not interpreted in this round. Preserve `segment_index` and do
not sort anything. A coarse row loses segment order, which is a reason the
canonical payload stays authoritative.

**Partial boundaries and strand.** `start_partial` / `end_partial` mean the real
feature extends beyond the sequence boundary. They do not change the stored
integers; surface them in the UI instead of widening the interval. `strand` is
stored once per location (-1, 0 or 1), not per segment.

### 6.2 Segment-level index (P1.1, P1.2)

If queries, matching or rendering need exact segment ranges, add either:

- a segment table `(feature_id, segment_index, start_pos, end_pos, site_kind,
  start_partial, end_partial, strand)` with a range index, or
- one range/GIN index over the projected `int8range[]` of the resolved
  segments.

Both derive from canonical segments resolved through
`Location::resolved_segments(length)`: each canonical segment maps to one row per
resolved piece, so a wrapped `span` becomes two rows and no segment row carries
`end_pos > length`. Each row carries the same `(start_pos, end_pos, site_kind)`
triple as the coarse row. This projection is the only representation that can
answer "does this query interval overlap an exon of this feature" without
over-matching cross-origin features, and it is where the resolved form of a
circular interval is materialized.

### 6.3 Feature identity (P1.5)

Platform feature ids are auto-increment integers assigned on insert. The
canonical record keeps no feature id. Within a record version, feature identity
is `features[]` source order, unique per record version. Key the platform id to
the immutable record version, never to the molecule: an edit creates a new record
version, and re-importing the same source with the same Sequon version produces
the same feature order and digests.

### 6.4 Identity digest (P1.4)

A platform column named `seq_digest` stores `digests.canonical`, the rotation-
and strand-invariant identity. `digests.sequence` may be stored separately (for
example as `sequence_digest`) but is not the identity. Two records are the same
molecule if and only if their canonical digests match.

A canonical digest identifies a **molecule**, not a stored row: several record
versions may share it (the same molecule imported from different files, or
re-imported after an annotation edit). Therefore `seq_digest` is an indexed
lookup, not a unique key. Give the payload its own unique identity - for
example the SHA-256 of the exact canonical JSON bytes - when re-import must be
idempotent. If the platform needs exactly one row per molecule, add a parent
molecule table keyed by `seq_digest` and store these records as versions below
it; do not enforce uniqueness on the digest column itself.

## 7. Worked example

The example binary in this repository exercises the whole consumer path:

```text
cargo run -p sequon-io --example platform-import
cargo run -p sequon-io --example platform-import -- testdata/genbank/070-unknown-key.gb
cargo run -p sequon-io --features snapgene --example platform-import -- testdata/snapgene/001-linear-uncompressed.dna
```

It reads a file (default: a committed GenBank fixture), calls
`sequon_io::detect` then `sequon_io::import`, verifies the report against the
payload and the source bytes, runs `Record::validate()` and
`sequon_algo::verify_digests()` for every record, and prints:

- records, diagnostics by severity and by code, dropped/preserved counts;
- per record: name, kind, topology, length, feature count, both digests and the
  canonical JSON payload size;
- the pin block: both schema ids and `$id`s, both schema bytes SHA-256 values
  computed over the library-embedded bytes (`sequon_model::RECORD_SCHEMA`,
  `sequon_io::REPORT_SCHEMA`), producer kind/name/version, source
  format/digest/bytes, export version, `codes_version` and digest algorithm.

It exits non-zero on a typed import error or a failed check, and it prints
source-derived text (record names) with control characters replaced and long
values truncated. Use it as the reference for the executor's accept step.

## 8. Consumer decisions: settled and open

These are consumer-side decisions. They do not change the payload contract and
are not blockers for using Sequon. Items marked decided state the rule inline;
the remaining items must be settled before the import route is advertised as
production-ready.

### 8.1 Schema, versions and pins

1. **Pin storage and enforcement.** Decided 2026-10-22. Store the pins with
   every import: schema `$id`, schema bytes SHA-256, producer version,
   `codes_version`, digest algorithm and source digest. A mismatch never blocks
   startup. Compare the producer version on load: a newer producer inside the
   compatibility window is upgraded during loading (re-import the retained
   original bytes with the newer reader, or migrate the stored payload); a
   producer outside the window requires a reader upgrade first. Sequon reports
   the versions so the platform can compare them (section 2).
2. **Reader/producer compatibility window.** Decided 2026-10-22: semver
   `major.minor.patch`. A reader R accepts a payload produced by P if and only
   if `P.major == R.major` and (`P.minor < R.minor`, or `P.minor == R.minor` and
   `P.patch <= R.patch + 1`). A producer on a higher minor, or on the same minor
   with a patch more than one ahead, requires a reader upgrade; a major bump may
   break compatibility and requires a migration with both versions coexisting
   (section 2.1).
3. **Upgrade procedure.** Decided 2026-10-22. On a Sequon upgrade, re-import the
   retained original bytes with the newer reader, or migrate the stored payload
   during loading. Canonical JSON bytes may change; canonical digests must not.
   A producer newer than the deployed reader is not a startup blocker while it
   stays inside the compatibility window.
4. **Non-Rust validation.** There is no language-neutral implementation of
   `Record::validate()` or the digest algorithm outside Rust. Decide whether
   all JSON consumption stays behind the Rust executor or a second
   implementation is risked. JSON Schema alone is insufficient (section 4).

### 8.2 Persistence and projection

5. **Canonical payload storage.** JSONB column, object store, or both; how the
   exact canonical JSON bytes are retained; whether projections are rebuilt
   from the payload.
6. **DDL mapping.** Decided 2026-10-22. Platform names: a `seq_digest` column
   stores `digests.canonical`, the rotation- and strand-invariant identity;
   `digests.sequence` may be stored separately (for example as
   `sequence_digest`) but is not the identity. `seq_digest` is an indexed
   molecule lookup, not a unique key: give the payload its own unique identity
   (for example the SHA-256 of the canonical JSON bytes) instead (section 6.4).
   `feature_key`, `start_pos`, `end_pos` and `site_kind` follow the projection
   in section 6; feature ids are auto-increment integers (section 6.3).
7. **P1 projection decision.** Decided 2026-10-22 (P1.1-P1.5, section 6),
   revised 2026-10-23 for circular cross-origin coordinates: the canonical
   payload already carries the unwrapped form for a single circular range, so a
   coarse row may copy that segment, while resolved split pieces come from
   `Location::resolved_segments(length)`. Circular records keep the unwrapped
   interval for a contiguous origin-crossing chain and resolve it on access; a
   non-contiguous join keeps the min/max superset with a segment post-filter;
   zero-length sites carry the `(start_pos, end_pos, site_kind)` triple; `order`
   is preserved but not interpreted; `seq_digest` stores the canonical digest;
   feature ids are auto-increment integers. Cover the committed fixtures for
   `join`/`complement` (020), `order` (030), origin-crossing (040) and partials
   (050) with tests.
8. **Feature identity.** Decided 2026-10-22. Platform feature ids are
   auto-increment integers assigned on insert; the canonical record keeps no
   feature id. Identity within a record version is `features[]` source order,
   unique per record version; an edit creates a new record version (section
   6.3).

### 8.3 Report, loss and diagnostics

9. **Report retention.** Store the full canonical report JSON, normalized rows,
   or both; how long; how to query diagnostics by code and record.
10. **Unknown codes and `codes_version`.** The platform's known-code table is
    the Sequon version it runs. Decide how to alert when a report's
    `codes_version` is newer than the supported table, and how unknown codes
    are surfaced without dropping them.
11. **`preserved[]` storage.** Base64 blocks are bounded by
    `max_preserved_block_bytes` (default 8 MiB) but still enlarge the report.
    Decide whether preserved bytes stay in the report, move to object storage
    with a reference, or are dropped from queries but retained for audit.
    Likewise decide `dropped[]` retention.
12. **Warning/dropped alerting.** Which warning codes or dropped reasons merit
    a user-visible notice, an operator alert, or only a stored count.

### 8.4 Import policy and transport

13. **Multi-record policy.** `ImportOptions.multiple` defaults to `All`. Decide
    per route whether an upload may contain several records (`All`), only the
    first is accepted (`First`, with the rest reported), or multiple records are
    rejected (`Reject`). Define the API/UI behavior for each.
14. **Hint policy.** Whether the platform supplies `name_hint`, `kind_hint` or
    `topology_hint` (for example from the original filename). Hints are used
    only when the format provides no value; a name taken from a filename is
    user-facing and must be an explicit product decision.
15. **Limits configuration.** Align `Limits` with upload limits and executor
    memory budget; map `LimitExceeded` to the platform's status code; keep the
    defaults unless a measured need justifies raising them.
16. **Error mapping and idempotency.** Map each `ImportError` code to the
    platform's HTTP/Job vocabulary, define 202/409 behavior, and choose the
    idempotency key (source digest plus import options plus producer version is
    the natural component). This is entirely platform-owned.
17. **Feature-gated formats.** Advertise SnapGene support only when the
    executor build enables the `snapgene` feature; otherwise detection still
    recognizes a `.dna` container and the import fails as `FormatUnknown`.
18. **Observability.** Metrics for severity counts, dropped/preserved counts,
    unknown-code counts, format and producer version, plus import latency and
    failure codes. Keep sequence content out of logs and traces.

## 9. Related documents

- [canonical-model.md](canonical-model.md) - payload semantics and D1-D16.
- [canonical-json-profile.md](canonical-json-profile.md) - byte rules and the
  determinism promises.
- [diagnostics.md](diagnostics.md) - code table, `codes_version`, unknown-code
  behavior, ordering and dropped/preserved vocabulary.
- `schemas/sequon-record-1.schema.json`,
  `schemas/import-report-1.schema.json` - machine-readable shapes.
- `crates/sequon-io/examples/platform-import.rs` - compiled consumer example.
