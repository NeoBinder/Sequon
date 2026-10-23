# Canonical model

Status: **accepted**, 2026-10-21, revised 2026-10-23 (circular cross-origin coordinates). Schema version 1: `sequon/record/1` for records and `sequon/import-report/1` for import reports.

This document is the semantic source of truth. `schemas/sequon-record-1.schema.json` and `schemas/import-report-1.schema.json` are its machine-readable copy, `docs/design/canonical-json-profile.md` fixes serialization, and `docs/design/diagnostics.md` owns the diagnostic vocabulary. When artifacts disagree, this document wins and the other artifacts change in the same commit.

Version 1 implements DNA only. `rna` and `protein` exist in the enum so later milestones do not force a new schema version; v1 parsers reject them with a typed error instead of guessing.

## Why this document exists

Every format we read (GenBank, SnapGene `.dna`) and every consumer we serve (storage, matching, visualisation) has to agree on one representation. Formats are adapters; the model is the truth. The adapters are ours: format IO is implemented and maintained in this repository, never delegated to an external parser crate or runtime. Getting this wrong is expensive to undo, so it is decided in writing first.

A record describes exactly one molecule at one point in time. Editing produces a new record; it never mutates the old one. Consumers may store records, index them, or transport them, but they must not need a second representation to interpret them.

## Model rules

1. **One sequence per record.** A record carries one DNA sequence, its normalised form inline, and its length.
2. **One coordinate system.** Every segment is 0-based and half-open. Conversion from a format happens once, in that format's adapter. The bounds are topology-dependent for a record of length `L`: a linear record keeps `0 <= start <= end <= L` in every segment, while a circular record's zero-length site keeps `0 <= start = end <= L` and its spanning segment satisfies `0 <= start < L` and `end <= start + L`. A circular span may therefore carry `end > L` for at most one origin crossing; a linear record never does.
3. **Qualifiers are ordered.** A feature's qualifiers are an ordered array of key/value pairs; duplicate keys and input order survive.
4. **Two identities.** `digests.sequence` identifies one normalised sequence; `digests.canonical` identifies the physical molecule across rotation and strand.
5. **Provenance is recorded, warnings are not.** Records carry where they came from and a digest of the source bytes. Parse warnings, dropped data and preserved blocks belong to the import report.
6. **Extensions are namespaced.** Format blocks the model cannot represent live in `extras` under the adapter's namespace.
7. **Loss is explicit.** Anything skipped is counted, located and explained; nothing is dropped silently.
8. **Determinism.** The same record and the same crate version serialize to the same bytes; the canonical digest is stable across parser versions.

## Record envelope

Required fields: `name`, `kind`, `topology`, `length`, `sequence`, `digests`, `features`, `provenance`, `extras`.
Optional fields: `description` and `molecule_type`, both nullable.

A field may be absent only when the schema declares it optional. Adding a new optional field is allowed within version 1; removing a field or changing its meaning requires version 2.

### Field order

Serialization follows schema declaration order:

- record: `schema`, `kind`, `molecule_type`, `topology`, `name`, `description`, `length`, `sequence`, `digests`, `features`, `provenance`, `extras`
- feature: `key`, `so_term`, `name`, `location`, `qualifiers`
- location: `operator`, `strand`, `segments`
- segment: `start`, `end`, `start_partial`, `end_partial`
- digests: `algorithm`, `sequence`, `canonical`
- provenance: `producer`, `source_format`, `source_digest`, `source_bytes`, `export_version`
- producer: `kind`, `name`, `version`
- extras: namespace keys sorted lexicographically; payloads keep their own documented order

### Example

The example below is illustrative. `schemas/examples/record.example.json` is the validated instance used by schema tests.

```json
{
  "schema": "sequon/record/1",
  "kind": "dna",
  "molecule_type": "ds-DNA",
  "topology": "circular",
  "name": "pUC19-CMV",
  "description": "synthetic reporter construct",
  "length": 72,
  "sequence": "ATGAGCAAAGGTGAAGAACTGTTTACCGGTGTGGTGCCGATCCTGGTGGAACTGGATGGTAACGGTAAATAA",
  "digests": {
    "algorithm": "sequon-digest/1",
    "sequence": "sha256:8d1653fb92457a81ac44417b35fcd789ec09ad0b60e6cbc687d63e9e6d4c4277",
    "canonical": "sha256:1cf59a87dc7cd3ad98b286e2b561a4e7820573ce91692fabdb297c106b4fd4b1"
  },
  "features": [
    {
      "key": "CDS",
      "so_term": "SO:0000316",
      "name": "gfp",
      "location": {
        "operator": "join",
        "strand": -1,
        "segments": [
          { "start": 62, "end": 72, "start_partial": false, "end_partial": false },
          { "start": 0, "end": 10, "start_partial": false, "end_partial": false }
        ]
      },
      "qualifiers": [
        { "key": "gene", "value": "gfp" },
        { "key": "db_xref", "value": "ASAP:ABCD1234" },
        { "key": "db_xref", "value": "GO:0003674" },
        { "key": "note", "value": null }
      ]
    }
  ],
  "provenance": {
    "producer": { "kind": "parser", "name": "sequon-io", "version": "0.1.0" },
    "source_format": "genbank",
    "source_digest": "sha256:5f1928f84e0a5429dfa4c533816aa87f9612de6e8c7bfe6ad101c904ea600d88",
    "source_bytes": 168,
    "export_version": null
  },
  "extras": {
    "genbank": {
      "header": [
        { "key": "ACCESSION", "value": "M77789.2" },
        { "key": "DEFINITION", "value": "synthetic reporter construct" }
      ]
    }
  }
}
```

The digests above are real: `sequence` is the SHA-256 of the normalised sequence, and `canonical` is the circular equivalence-class digest described below.

## kind, topology, molecule_type and required sources

`name`, `kind` and `topology` never have a default. Each must come from a format field or a caller hint; when neither provides it, the import fails with a typed error. A caller hint is consulted only when the format does not provide the value and never overrides parsed data.

| Field | Values | Required | GenBank source | SnapGene source | When missing |
| --- | --- | --- | --- | --- | --- |
| `name` | non-empty string | yes | `LOCUS` name | notes `AccessionNumber`, else `CustomMapLabel` when `UseCustomMapLabel` is 1, else plain `Comments`; or `name_hint` | `RECORD_REQUIRED_FIELD_MISSING { field: "name" }` |
| `kind` | `dna` \| `rna` \| `protein` | yes | `LOCUS` molecule type, or `kind_hint` | cookie sequence type, or `kind_hint` | `RECORD_REQUIRED_FIELD_MISSING { field: "kind" }` |
| `topology` | `linear` \| `circular` | yes | `LOCUS` topology, or `topology_hint` | DNA flags bit 0, or `topology_hint` | `GB_TOPOLOGY_MISSING` / `DNA_TOPOLOGY_MISSING`; unknown value is `RECORD_FIELD_INVALID` |
| `molecule_type` | `ds-DNA` \| `ss-DNA` \| `other` \| null | no | `LOCUS` subtype | not represented; remains null | unknown subtype becomes `other` with `MOLECULE_TYPE_UNKNOWN` |

Rules:

- `kind = dna` is the only kind v1 produces. A GenBank record declaring mRNA/RNA/protein fails with `GB_UNSUPPORTED_MOLECULE`; a `.dna` file whose cookie sequence type is not DNA fails with `DNA_UNSUPPORTED_MOLECULE` (unknown values: `DNA_UNKNOWN_SEQ_TYPE`). There is no downgrade to DNA.
- `molecule_type` is optional because many formats do not state it. The GenBank writer derives `ds-DNA` from `kind = dna` when it is null and records `WRITE_MOLECULE_TYPE_DERIVED` (info). The value never changes the digest.
- `kind = rna` or `protein` is schema-valid for future milestones but is out of scope for v1 adapters.

## Sequence and length

- `sequence` is the normalised sequence: uppercase, no whitespace, no digits, IUPAC DNA characters `ACGTNRYSWKMBDHV`. It is never empty.
- `length` is the number of bases and must equal `sequence` length. The redundancy is an integrity check, not a second source of truth.
- Normalisation happens once, in the adapter, and is reported there (`GB_SEQUENCE_NORMALIZED`, `GB_CRLF_NORMALIZED`). The canonical record stores only the normalised form.
- A DNA record containing `U` fails with `GB_RNA_BASE_IN_DNA`; any other character outside the IUPAC set fails with `GB_ORIGIN_INVALID_CHAR` and the source line.
- More than `max_sequence_len` is a `LimitExceeded` typed error; sequences are never truncated.
- v1 does not stream. The whole input is in memory and every allocation is checked against the limits first.

## Digests

`digests.algorithm` is `sequon-digest/1` for version 1. Both digests are `sha256:` followed by 64 lowercase hexadecimal characters.

- `digests.sequence = sha256(sequence)`. It is direction-sensitive: it identifies one normalised entry, not one physical molecule.
- `digests.canonical` is the equivalence-class identity:
  - linear: `sha256(min_lex(sequence, revcomp(sequence)))`;
  - circular: `sha256(min_lex(min_rotation(sequence), min_rotation(revcomp(sequence))))`.
- `revcomp` uses the IUPAC complement map `A<->T`, `C<->G`, `R<->Y`, `S<->S`, `W<->W`, `K<->M`, `B<->V`, `D<->H`, `N<->N` and reverses the string. `min_rotation` is the lexicographically smallest rotation (Booth's algorithm).
- The canonical digest depends only on the normalised sequence, `topology` and the algorithm id. It does not depend on parser version, field order, extras, warnings or caller hints.
- The recorded sequence keeps its input direction. Revcomp and rotation are only used to compute the equivalence class; they never rewrite the record.
- Deterministic serialization is promised only for the same crate version. `digests.canonical` is the permanent identity across parser versions. The two promises are separate and neither implies the other.
- Test vectors live in `tests/digest-vectors.json` and must cover linear revcomp, circular rotation, circular revcomp, and `N`/IUPAC input. The computation lives in `sequon-algo`; `sequon-model` only holds the value type.

## Features and locations

A feature has:

- `key`: the source feature key, kept verbatim (unknown keys are not rewritten to `misc_feature`);
- `so_term`: optional static mapping from common GenBank keys to Sequence Ontology terms; unknown keys get `null`, never a guess;
- `name`: optional display name from the source. The GenBank adapter uses the first `/gene` qualifier value when present and `null` otherwise (never `/product`, never synthesized); the SnapGene adapter uses the feature's `name` attribute (absent or empty becomes `null`);
- `location`: see below;
- `qualifiers`: ordered key/value pairs, see below.

### Location

```json
{ "operator": "span", "strand": 1, "segments": [ { "start": 0, "end": 10, "start_partial": false, "end_partial": false } ] }
```

- `operator` is `span`, `join` or `order`; `strand` is `-1`, `0` or `1`.
- `span` carries exactly one segment. `join` and `order` carry one or more segments in source order.
- Each segment is 0-based and half-open. The bounds depend on `topology` for a record of length `L`: on a linear record every segment satisfies `0 <= start <= end <= L`; on a circular record a zero-length site satisfies `0 <= start = end <= L` and a spanning segment satisfies `0 <= start < L` and `end <= start + L`. A zero-length segment (`start == end`) is only legal for a GenBank `n^m` between-positions location and carries `GB_LOCATION_BETWEEN_POSITIONS` (info). A zero-length site never wraps.
- `start_partial` / `end_partial` preserve GenBank `<` / `>` markers on the corresponding boundary of each segment.
- `complement(...)` changes only `strand`; segment coordinates are not flipped. `join` and `order` keep their segments in source order; `order` is never sorted.
- **A circular origin-crossing range is stored unwrapped.** A single range that crosses the origin of a circular record is one segment with `end > length`, at most one wrap. For a 6000 bp circular record, GenBank `5900..6100` (1-based closed) becomes the 0-based half-open segment `{5899, 6100}` under `operator = span`; `6100 <= 5899 + 6000` holds, so it wraps exactly once. The wrap is resolved on access, not at parse time: `Location::resolved_segments(length)` (and `Segment::resolved(length)`) return the origin-ordered pieces `[5899, 6000) + [0, 100)`, keeping `start_partial` on the first piece and `end_partial` on the second. The writer re-serializes that canonical segment as `5900..6100`. No base is lost in either direction.
- **An explicitly split source stays split.** An explicitly split source, such as GenBank `join(5900..6000,1..100)`, stays two segments `[{5899, 6000}, {0, 100}]` under `operator = join`; its segments are never merged into a span, and a span is never split at the origin. The writer re-serializes that split form as `join(5900..6000,1..100)`.
- There is exactly one representation per source syntax: a single range is one segment (unwrapped when it crosses a circular origin) and an explicit `join`/`order` keeps its segments in source order. An adapter normalizes once; consumers never see both the unwrapped and the split form of the same source range. Consumers that need origin-ordered pieces call `resolved_segments(length)` instead of splitting coordinates themselves.
- Unsupported forms in v1 (`one-of`, `replace`, remote accessions, `telomere`, `gap`) do not abort the file: the feature is skipped, reported in `dropped[]` and gets `GB_LOCATION_UNSUPPORTED` (warning). Linear coordinates that exceed the record length but stay within `max_sequence_len` follow D11 the same way (`GB_COORDINATE_OUT_OF_RANGE`, warning, feature skipped).
- Syntactically malformed locations (unbalanced parentheses, non-numeric bounds) are structural errors: the file aborts with `GB_LOCATION_MALFORMED` (error, with line and column). Coordinates beyond `max_sequence_len` are a `LimitExceeded` typed error. A structurally broken record (LOCUS/ORIGIN/container) always aborts.
- Location depth above `max_location_depth` or segments above `max_segments_per_feature` is a `LimitExceeded` typed error.

## Qualifiers

`qualifiers` is an array, not an object:

```json
[
  { "key": "gene", "value": "gfp" },
  { "key": "db_xref", "value": "ASAP:ABCD1234" },
  { "key": "db_xref", "value": "GO:0003674" },
  { "key": "note", "value": null }
]
```

- Duplicate keys and input order are preserved; GenBank permits repeated `/db_xref` and `/note` and a map would lose them.
- `value` is a string or `null`. `null` is a bare qualifier such as `/pseudo`.
- There is no whitelist and no rewriting of semantic qualifiers (`/translation`, `/product`, `/codon_start`, `/transl_table` are strings).
- Convenience accessors may exist for lookup, but they read this array; they are not a second store.
- The GenBank writer quotes values and replaces an embedded double quote with two apostrophes (`''`) following GenBank convention, recording `WRITE_QUOTE_NORMALIZED` (info). The substitution is one-way and documented: the stored record keeps the original character, while a re-import of the writer output yields `''`.

## Provenance

```json
{
  "producer": { "kind": "parser", "name": "sequon-io", "version": "0.1.0" },
  "source_format": "genbank",
  "source_digest": "sha256:...",
  "source_bytes": 168,
  "export_version": null
}
```

- `producer.kind` is `parser`, `algorithm` or `manual`. v1 importers emit `parser`; algorithm and manual producers are reserved for later edited/derived records.
- `producer.name` is `sequon-io` for format adapters; `producer.version` is the crate version that parsed the bytes.
- `source_format` is `genbank` or `snapgene-dna`.
- `source_digest` is the SHA-256 of the raw input bytes and `source_bytes` is their length. This lets a consumer re-parse the original bytes later.
- `export_version` is the SnapGene export version as an unsigned 32-bit integer; it is `null` for GenBank. Values beyond `u32` are rejected at parse time.
- **No warnings.** Parse diagnostics are events and live only in the import report. A record copied into a new version does not inherit the old file's parse warnings.

## Extras

`extras` holds format payloads the canonical subset cannot express. It is a JSON object keyed by adapter namespace: `genbank` and `snapgene` in v1.

- Each adapter writes only its own namespace and reads only its own namespace for export.
- Unknown namespaces are preserved in structure and value; the canonical JSON profile re-emits their object keys in sorted order, so preservation is semantic rather than byte-for-byte. If two adapters write the same namespace, the import reports `EXTRAS_NAMESPACE_COLLISION` (warning).
- `extras` never affects `digests.canonical`. The core does not interpret it; it validates and passes it through.
- `extras.genbank.header` is an ordered array of `{ "key", "value" }` pairs carrying every header field (ACCESSION, VERSION, DEFINITION, ORGANISM and its taxonomy continuation, SOURCE, REFERENCE, COMMENT, KEYWORDS and unknown fields). `DEFINITION` also populates `description`. Header order is preserved so export is lossless.
- `extras.genbank.division` is an optional convenience copy of the LOCUS division token so export can restore it; it never replaces the header list and is not a second header.
- `extras.snapgene` holds decoded or structured metadata only: notes text, sequence properties, primer metadata, feature display attributes and history-tree metadata as documented in `snapgene-dna.md`. Raw preserved bytes (history trees, trace containers, unknown packets) live only in the import report's `preserved[]` or `dropped[]`, following D7; the plan's earlier `extras.snapgene.history` base64 example is superseded and is not part of this contract.

## Import report

An import returns records plus one report. The report is an event document: it is never written back into a record, and a report may cover more than one record.

```json
{
  "schema": "sequon/import-report/1",
  "source_format": "snapgene-dna",
  "source_digest": "sha256:...",
  "source_bytes": 481207,
  "parser": { "kind": "parser", "name": "sequon-io", "version": "0.1.0" },
  "export_version": 17,
  "codes_version": "2026-10-21/1",
  "records": [
    { "index": 0, "name": "pET28a-SUMO-hGLP1", "kind": "dna", "length": 6142,
      "sequence_digest": "sha256:...", "canonical_digest": "sha256:..." }
  ],
  "diagnostics": [
    { "code": "DNA_HISTORY_PRESERVED_RAW", "severity": "info",
      "message": "history tree kept as raw bytes; not mapped",
      "position": { "kind": "snapgene", "offset": 40960, "packet_type": 7, "packet_index": 12 },
      "record_index": 0 }
  ],
  "dropped": [
    { "what": "trace_container", "reason": "size_limit", "packet_type": 16,
      "offset": 350000, "length": 12000, "digest": "sha256:...", "record_index": 0 }
  ],
  "preserved": [
    { "what": "unknown_packet", "encoding": "base64", "data": "...",
      "packet_type": 42, "offset": 360000, "length": 64, "record_index": 0 }
  ]
}
```

- `records[]` summarises returned records. `index` is the 0-based parse-order ordinal; it ties diagnostics, dropped entries and preserved entries to a record. Under `multiple = First` the remaining records are still parsed for diagnostics, so their `record_index` may not appear in `records[]`; consumers must treat `record_index` as the source ordinal, not an array offset.
- `diagnostics[]` carries `code`, `severity`, `message`, an optional `position` and an optional `record_index`. Order is stable: sorted by `(position.offset or line, code)`. Messages never contain user sequence content, only positions and lengths.
- `codes_version` identifies the diagnostic vocabulary used by this report. It is required in version 1 so consumers can map unknown codes without rejecting the report.
- `dropped[]` lists skipped data by fingerprint: `what` and `reason` are required; `record_index`, `position`, `packet_type`, `offset`, `length`, `digest` are optional. GenBank feature skips also use `feature_key` and `location_text`. Dropped entries keep identity and reason, never the payload.
- `preserved[]` lists data kept verbatim (base64) within `max_preserved_block_bytes`. An entry has `what`, `encoding = "base64"`, `data` and optional location/fingerprint fields.
- `dropped` and `preserved` are mutually exclusive: a block is either kept (with its bytes in `preserved`) or dropped (with only a digest in `dropped`).
- A structural failure that leaves no usable record returns `ImportError` and no report. In a multi-record import, a record that fails while other records remain usable may appear as an error-severity diagnostic carrying its `record_index`; the import still returns the usable records. Lossy per-feature outcomes return records plus warnings plus `dropped` entries.
- Multi-record policy (D10): `All` returns every record, `First` returns the first and reports the rest, `Reject` fails with `GB_MULTIPLE_RECORDS_NOT_ALLOWED`.

## Validation

JSON Schema validates structure, enums, patterns and tuple arity. It cannot compare two fields, so `Record::validate()` owns the cross-field invariants:

- `length` equals `sequence` length;
- the topology-aware segment bounds: on a linear record `0 <= start <= end <= length` for every segment; on a circular record a zero-length site keeps `0 <= start = end <= length` and a spanning segment satisfies `0 <= start < length` and `end <= start + length` (at most one wrap);
- `operator = span` has exactly one segment;
- `digests.sequence` and `digests.canonical` syntax is checked by `validate()`; their values are recomputed by `sequon-algo::verify_digests()`. Consumers call both, because the canonical model crate does not depend on a hashing crate;
- `name` is non-empty and `provenance` is internally consistent (`source_bytes` non-negative, digest prefixed);
- no floating-point numbers appear anywhere in the document;
- an adapter's `extras` namespace is written only by that adapter.

Consumers call `validate()` after import and before persisting. A failed validation is a typed error naming the field and, where available, the source position. Schema validation alone is never sufficient.

## Versioning and compatibility

- `sequon/record/1` and `sequon/import-report/1` are fixed identifiers. Optional additive fields keep version 1; removing a field or changing its meaning requires version 2, and version 1 stays available for stored data.
- The schemas carry stable `$id` values (`urn:sequon:schema:record:1`, `urn:sequon:schema:import-report:1`). The exact schema bytes ship inside the library as `sequon_model::RECORD_SCHEMA` and `sequon_io::REPORT_SCHEMA`, with the `$id` constants `sequon_model::RECORD_SCHEMA_URN` and `sequon_io::REPORT_SCHEMA_URN`; consumers pin the `$id`, the SHA-256 of those bytes and the producer version, and they do not copy the schema body. A published copy under `schemas/` serves non-Rust consumers. The embedded bytes are frozen per crate release, so a schema change requires a crate release.
- Version 1 readers are strict: they reject unknown envelope fields. An additive optional field changes the embedded schema bytes and their SHA-256; that hash, the `$id` and the producer version are how a consumer detects version skew. Detection never refuses startup: consumers store the pins with the payload and compare versions when loading it.
- **Compatibility window (semver).** A reader R accepts a payload produced by producer P if and only if `P.major == R.major` and (`P.minor < R.minor`, or `P.minor == R.minor` and `P.patch <= R.patch + 1`). A producer on a higher minor, or on the same minor with `P.patch > R.patch + 1`, requires a reader upgrade first. A major bump may break compatibility and requires a migration with both versions coexisting while stored data is upgraded. Within the window, a newer producer never blocks startup: the data is upgraded during loading, by re-importing the retained original bytes with the newer reader or by migrating the stored payload.
- `digests.algorithm` is independently versioned. A semantic change to the digest definition requires a new algorithm id; old digests remain recomputable.
- The diagnostic vocabulary is append-only within a `codes_version`. A new code raises `codes_version`; it never changes the meaning of an existing code. Consumers map unknown codes to `unknown`, keep the raw code and do not reject the report.
- M0 clarifications encoded here: `codes_version` is required in the report; `dropped[]` carries optional GenBank feature fields; cross-field invariants belong to `validate()`; `extras` is the only forward-compatible extension point at envelope level.

## Out of scope for version 1

- `.rna` / `.prot` parsing and export; FASTA; A3, PEFF, SBOL3, GFF3.
- SnapGene `.dna` writing or any SnapGene binary output.
- Matching, editing, construction simulation, restriction analysis and annotation algorithms (these belong to `sequon-algo` later).
- Database tables, HTTP routes, permissions, job orchestration and UI graphs.
- A CLI distribution layer; example binaries for validation do not count as one.
- Streaming input interfaces; version 1 uses `&[u8]` and explicit limits.

## Open items

| # | Item | Status | Owner |
| --- | --- | --- | --- |
| Q8 | Can SnapGene "Save As GenBank" outputs be committed? | Provisional: `paired/` stays local, schemas and tests use synthetic fixtures | lead + legal |
| Q9 | XML reader: `quick-xml` versus a hand-written pull parser | Resolved at M0 in `snapgene-dna.md` after registry verification | contract + snapgene |
| Q10 | MSRV (dual MIT/Apache-2.0 licensing was declined on 2026-10-22; the workspace stays MIT) | Before first release; MSRV = two stable minors behind at that time | lead |
| P1 | Platform projection of cross-origin features into single-row `start_pos`/`end_pos` | Decided 2026-10-22, clarified 2026-10-23: the canonical payload itself stores the unwrapped form for a single circular origin-crossing range (one segment with `end > length`, resolved on access); an explicitly split source stays split. A circular record's stored interval may keep a contiguous origin-crossing chain unwrapped and resolve the wrap on access; zero-length sites carry a `(start_pos, end_pos, site_kind)` discriminator; `order` is not interpreted this round; `seq_digest` stores the canonical digest; platform feature ids are auto-increment integers. Canonical `segments` stay authoritative and the library assumes no platform row shape. | lead + platform |

Closed: Q1-Q7, Q11, Q12, Q14 (see the task plan `development-plan.md` section 12.2), P1 (2026-10-22) and the digest/kind/required-field/API-shape decisions below.

## Decision log

All decisions below were accepted on 2026-10-21, except D2 as revised on 2026-10-23. S1-S9 in the task plan are the interaction decisions that produced them.

| ID | Decision |
| --- | --- |
| D1 | Schema ids `sequon/record/1` and `sequon/import-report/1`; payload schema belongs to Sequon, transport to the consumer. |
| D2 | 0-based half-open coordinates; `strand in {-1,0,1}`; location is `operator + segments[]`. Revised 2026-10-23 (circular cross-origin coordinates): the canonical payload stores a single circular origin-crossing range unwrapped as one segment with `end > length` (one wrap, resolved on access), while an explicitly split source stays split; linear records are unchanged and never carry `end > length`. This supersedes only the earlier "cross-origin locations are split; `end > length` never appears in a record" clause, and only for circular records; see the Location section for the rule and the worked example. |
| D3 | Qualifiers are an ordered key/value array preserving duplicate keys and input order. |
| D4 | Two digests: `sequence` and `canonical`; linear uses revcomp equivalence, circular adds rotation; algorithm id `sequon-digest/1`. |
| D5 | Byte-identical JSON only within one crate version; `canonical` digest is the permanent cross-version identity. |
| D6 | Provenance records producer, source format, source digest, source bytes and SnapGene export version; it never records warnings. |
| D7 | Warnings, dropped data and preserved blocks live only in the import report; a new record version does not inherit them. |
| D8 | `extras` is namespace-isolated per adapter; each adapter writes only its own namespace. |
| D9 | Every GenBank header field is preserved in order in `extras.genbank.header`; `description` remains a first-class field. |
| D10 | Multi-record GenBank is supported; `multiple` is `First`, `All` (default) or `Reject`. |
| D11 | Structural errors abort; an inexpressible feature is skipped with `dropped` plus a warning. |
| D12 | Limits have documented defaults, are configurable, and always produce typed errors instead of truncation. |
| D13 | `.dna` import lives behind the `snapgene` feature of `sequon-io` using general-purpose codecs only. |
| D14 | No SnapGene writer; history trees are preserved and validated, not mapped into a family tree. |
| D15 | `kind` enum is `{dna, rna, protein}` in the schema; v1 implements only DNA. |
| D16 | `name`, `kind` and `topology` require an explicit source; `molecule_type` is optional and derived on export. |
