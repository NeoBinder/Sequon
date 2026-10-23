# Canonical JSON profile

**Status: accepted, M0 contract.** This document fixes the byte-level form of
every JSON document Sequon produces for the canonical record (`sequon/record/1`)
and the import report (`sequon/import-report/1`). Field-level semantics live in
[canonical-model.md](canonical-model.md); the machine-readable copy is
`schemas/sequon-record-1.schema.json` and `schemas/import-report-1.schema.json`.
Diagnostic codes and their ordering inputs live in
[diagnostics.md](diagnostics.md).

Two different promises are separated on purpose (D4/D5):

- **Byte stability** is promised only for a fixed Sequon crate version: the same
  input bytes and options produce the same output bytes.
- **Canonical digests** are permanent identities: they do not change when a
  parser version, this profile, field order or `extras` content changes.

## 1. Encoding

- A canonical document is one JSON value, encoded as UTF-8 without a byte order
  mark.
- Line ends are LF (`0x0A`). CR is never emitted, including inside multi-line
  string values, which use the two-character escape for a line feed.
- Tabs are never emitted, inside or outside strings. Indentation is spaces only.
- One JSON document is one object at the top level. Duplicate object keys are
  never emitted.
- A `.json` file ends with exactly one newline after the root value: no blank
  line, no trailing spaces, and no second newline. A serializer that writes to a
  byte buffer without a newline is not writing a file; the writer adds exactly
  one.

## 2. Field order

Field order is the declaration order of the corresponding JSON Schema object.
It is fixed by [canonical-model.md](canonical-model.md); the tables below are the
operational copy. A serializer emits fields in this order regardless of the
order in which a struct declares them and regardless of any map iteration order.

`sequon/record/1`:

| Position | Field | Presence |
| --- | --- | --- |
| 1 | `schema` | required |
| 2 | `kind` | required |
| 3 | `molecule_type` | nullable, always emitted |
| 4 | `topology` | required |
| 5 | `name` | required |
| 6 | `description` | nullable, always emitted |
| 7 | `length` | required |
| 8 | `sequence` | required |
| 9 | `digests` | required |
| 10 | `features` | required |
| 11 | `provenance` | required |
| 12 | `extras` | required |

Nested objects:

| Object | Field order |
| --- | --- |
| `digests` | `algorithm`, `sequence`, `canonical` |
| `features[]` | `key`, `so_term`, `name`, `location`, `qualifiers` |
| `location` | `operator`, `strand`, `segments` |
| `segments[]` | `start`, `end`, `start_partial`, `end_partial` |
| `qualifiers[]` | `key`, `value` |
| `provenance` | `producer`, `source_format`, `source_digest`, `source_bytes`, `export_version` |
| `producer` | `kind`, `name`, `version` |
| `extras` | sorted keys; see section 4 |

`sequon/import-report/1`:

| Position | Field | Presence |
| --- | --- | --- |
| 1 | `schema` | required |
| 2 | `source_format` | required |
| 3 | `source_digest` | required |
| 4 | `source_bytes` | required |
| 5 | `parser` | required |
| 6 | `export_version` | nullable, always emitted |
| 7 | `codes_version` | required |
| 8 | `records` | required |
| 9 | `diagnostics` | required |
| 10 | `dropped` | required |
| 11 | `preserved` | required |

Nested objects:

| Object | Field order |
| --- | --- |
| `parser` | `kind`, `name`, `version` |
| `records[]` | `index`, `name`, `kind`, `length`, `sequence_digest`, `canonical_digest` |
| `diagnostics[]` | `code`, `severity`, `message`, `position`, `record_index` |
| `position` (GenBank) | `kind`, `line`, `column` |
| `position` (SnapGene) | `kind`, `offset`, `packet_type`, `packet_index` |
| `dropped[]` | `what`, `reason`, `record_index`, `position`, `packet_type`, `offset`, `length`, `digest`, `feature_key`, `location_text` |
| `preserved[]` | `what`, `encoding`, `data`, `packet_type`, `offset`, `length`, `digest`, `record_index` |

Arrays keep canonical order and are never sorted by the serializer:

- `features` in the order the adapter produced them (source order),
- `qualifiers` in source order, duplicate keys preserved,
- `location.segments` in the order the location operator defines (`join` and
  `order` both keep the written order; `order` is never sorted),
- `extras.genbank.header` in source order,
- `records` in output order, `dropped` and `preserved` in discovery order.

## 3. Optional and nullable fields

Two different mechanisms exist and must not be confused:

- **Nullable fields are always emitted**, as JSON `null` when the value is
  unknown or not applicable. Presence therefore never depends on the content,
  which keeps byte determinism independent of optionality.
  In `sequon/record/1`: `molecule_type`, `description`, `features[].so_term`,
  `features[].name`, `provenance.export_version`.
  In `sequon/import-report/1`: `diagnostics[].position`,
  `diagnostics[].record_index`, and `export_version` (null for GenBank).
- **Non-nullable optional fields are omitted when unknown.** Every optional
  field of `dropped[]` and `preserved[]` is in this class (`record_index`,
  `position`, `packet_type`, `offset`, `length`, `digest`, `feature_key`,
  `location_text`). They are never emitted as `null`.

Schema validation accepts the absence of both classes, so a hand-authored
document that omits a nullable field is still valid. The canonical serializer
never omits one: a document produced by Sequon always contains the nullable
fields.

## 4. `extras`

`extras` is the only forward-compatible extension point at the envelope level,
and it is isolated by adapter namespace (`genbank`, `snapgene`):

- Object keys inside `extras` are sorted lexicographically (byte order) at
  every nesting level, which is what a `BTreeMap`-backed representation
  produces. This is the one place where key order is not schema declaration
  order.
- Arrays inside `extras` keep their element order.
- An adapter writes only its own namespace. An unknown namespace found in an
  input document is preserved: its structure, scalar values and array order
  survive a round trip, and its keys are re-emitted in sorted order, not in the
  byte order they arrived in.
- Core code never interprets `extras`, and `extras` never contributes to a
  digest.
- Once a namespace exists, its shape is owned by the adapter named after it and
  documented with that adapter.

## 5. Numbers

- There is no floating-point number anywhere in either document: no field has a
  fractional value, and no serializer may emit one.
- Integers are base 10, unsigned, without leading zeros, without a plus sign,
  without an exponent, and without a decimal point. Zero is written `0`, never
  `-0`, `0.0` or `0e0`.
- `length`, `source_bytes`, `start`, `end`, `offset`, `line`, `column`,
  `packet_type`, `packet_index`, `index` and `export_version` are integers;
  `strand` is the only signed field and accepts `-1`, `0`, `1`.
- JSON Schema `"type": "integer"` also accepts `1.0` because draft 2020-12
  compares the mathematical value. The profile is stricter than the schema:
  `1.0` is not canonical output. A reader that must reject non-canonical number
  syntax checks the input text, not the parsed value.
- Booleans are `true` and `false`; zero and one are never used for a boolean.

## 6. Strings and escaping

String content is emitted in its shortest JSON form:

- Escape only what JSON requires: the double quote, the backslash, and control
  characters below `0x20`.
- Use the short escapes where they exist: backspace, form feed, line feed,
  carriage return and tab. Any other control character uses the lowercase-hex
  Unicode escape form (backslash, `u`, four lowercase hex digits).
- Do not escape the solidus, the angle brackets, the ampersand, the apostrophe,
  or any character that does not need escaping. HTML-safe escaping is not
  applied: this is data, not markup.
- Non-ASCII text is emitted as raw UTF-8, never as escaped surrogate pairs.
  `U+2028` and `U+2029` are emitted raw because JSON does not require
  escaping them.
- A string value may contain a newline only as the two-character line-feed
  escape; a literal LF inside a JSON string is not canonical. This is why a
  multi-line GenBank header value is still a single JSON string.
- `sequence` is one JSON string with no line breaks and no whitespace. It is
  never wrapped to a line width; only the JSON indentation around it changes
  lines.
- No Unicode normalization is applied. Strings are preserved as the adapter
  produced them; two visually identical strings in different normalization
  forms stay different values, and the digest of a sequence is computed over
  the exact characters.

## 7. Pretty-printing

Documents are pretty-printed for review and diff stability:

- Two spaces per nesting level, no tabs.
- One member per line, key, colon, exactly one space, then value.
- One array element per line.
- Empty objects and arrays are `{}` and `[]`, never split over two lines.
- No trailing whitespace on any line.
- Exactly one final newline, as stated in section 1.

## 8. Determinism

- For one Sequon crate version and one feature set, the same input bytes and
  import or write options produce byte-identical output. Two imports of the same
  bytes must also produce the same `diagnostics` sequence.
- Determinism never depends on hash map iteration order, wall-clock time,
  environment variables, locale, or the current working directory. Every value
  that influences output is either derived from the input or recorded in
  `provenance` (producer kind/name/version, source format, source digest, source
  bytes, export version) - never in `diagnostics` and never in `extras` prose.
- Importing the same file twice yields the same `Record` and the same diagnostics
  order; the JSON text is identical.
- Diagnostics are emitted in a deterministic order defined in
  [diagnostics.md](diagnostics.md): positioned events first in ascending source
  order (GenBank line/column, SnapGene offset/packet), then `code` in ascending
  byte order, then emission order as the stable tie-break. A stable sort over a
  deterministic emission order is required; an unstable sort is a bug.
- `canonical` digests are computed from the normalized sequence and topology
  only, following `sequon-digest/1`: linear records use
  `min(sequence, reverse_complement(sequence))`, circular records use
  `min(min_rotation(sequence), min_rotation(reverse_complement(sequence)))`.
  Neither the parser version, this profile, field order, nor `extras` may change
  them (D5).
- Digest vectors and their expected values live at the repository root in
  `tests/digest-vectors.json`.

## 9. Example

The following is the complete canonical serialization of a small linear record.
It is compact only because the sequence is short; every rule above applies.
Every digest is reproducible from the inputs: `sequence` and `canonical` are
computed over the shown sequence, and `source_digest` is the sha256 of the
GenBank source shown underneath the example.

```json
{
  "schema": "sequon/record/1",
  "kind": "dna",
  "molecule_type": null,
  "topology": "linear",
  "name": "SHORT-EX",
  "description": "short synthetic example.",
  "length": 25,
  "sequence": "TGCAAGCTTGCAAGCTTGCAAGCTT",
  "digests": {
    "algorithm": "sequon-digest/1",
    "sequence": "sha256:e03e77d66b5264f3866f986591d4caadc045f03d0e6ba7de1a38ebe5402f9582",
    "canonical": "sha256:9eb3ac2d939b083afea3379bd41902a930951eff1cf20355ede53fac7f917a20"
  },
  "features": [
    {
      "key": "CDS",
      "so_term": "SO:0000316",
      "name": "short",
      "location": {
        "operator": "span",
        "strand": 1,
        "segments": [
          {
            "start": 0,
            "end": 25,
            "start_partial": false,
            "end_partial": false
          }
        ]
      },
      "qualifiers": [
        {
          "key": "gene",
          "value": "short"
        }
      ]
    }
  ],
  "provenance": {
    "producer": {
      "kind": "parser",
      "name": "sequon-io",
      "version": "0.1.0"
    },
    "source_format": "genbank",
    "source_digest": "sha256:1716ae73e5360a07d25e9ba0907e8da703246de795af8dc5ee188b25ecdd7142",
    "source_bytes": 287,
    "export_version": null
  },
  "extras": {
    "genbank": {
      "header": [
        {
          "key": "ACCESSION",
          "value": "SHORT-EX"
        }
      ]
    },
    "unknown_example": {
      "alpha_note": "kept verbatim",
      "zeta": 1
    }
  }
}
```

The 287 source bytes behind `provenance.source_digest` (GenBank, LF line ends,
one final newline):

```text
LOCUS       SHORT-EX                25 bp    DNA     linear   SYN 01-JAN-2000
DEFINITION  short synthetic example.
ACCESSION   SHORT-EX
FEATURES             Location/Qualifiers
     CDS             1..25
                     /gene="short"
ORIGIN
        1 tgcaagcttg caagcttgca agctt
//
```

Notes on the example:

- The canonical digest is the digest of the reverse complement, because
  `AAGCTTGCAAGCTTGCAAGCTTGCA` sorts before the recorded orientation. The
  recorded orientation is still what `sequence` holds: the digest never rewrites
  a record.
- `molecule_type` and `provenance.export_version` show the nullable rule:
  present, with `null` where the value is unknown. `features[0].so_term` comes
  from the static key table and `features[0].name` from `/gene`, so both carry
  values here; they would be `null` if the key had no mapping or the qualifier
  were absent.
- `unknown_example` is an unknown namespace, kept verbatim with its keys sorted.

The larger examples in `schemas/examples/record.example.json` and
`schemas/examples/report.example.json` use a self-authored 2686 bp sequence: a
fixed-seed SHA-256 expansion over A/C/G/T with one IUPAC ambiguity code every
251st position. The sequence is not derived from any organism or database entry,
and the examples are schema examples, not conformance fixtures. Their `sequence`
and `canonical` digests are reproducible from the shown sequence with the
algorithm above; their source digests describe synthetic source documents that
are not stored in this repository, so they remain illustrative until M1/M4
fixtures exist.

## 10. What this profile does not promise

- **No byte identity across crate versions.** A new version may add an optional
  field, change this profile, or change diagnostic wording, so compare canonical
  digests and schema-valid content, never bytes, across versions (D5).
- **No byte identity for documents produced by other tools** or for documents
  that were round-tripped through a generic JSON parser and printer. The profile
  describes Sequon output.
- **No guarantee that key order survives a foreign JSON implementation.**
  Unknown `extras` namespaces are preserved in structure and value, not in their
  original byte order.
- **No Unicode normalization or case folding of any string.** Values are stored
  as read.
- **No floating-point support.** A document containing `1.0` where an integer is
  required is not canonical output even if a schema validator accepts it.
- **No stability of diagnostic `message` text.** Codes, severities and positions
  are the contract; messages are human-readable and may be reworded without a
  code table bump.
- **No guarantee that `extras` content is interpretable by core code.** Only the
  adapter that owns a namespace reads it.
- **No line wrapping of long strings.** A long sequence stays one JSON string on
  one line; diff tools may struggle, and that is accepted.

## 11. Related documents

- [canonical-model.md](canonical-model.md) - field semantics and decisions
  D1-D16/S1-S9, the source of truth for field order.
- [diagnostics.md](diagnostics.md) - code table, severity, positions,
  `codes_version`, and the exact diagnostic sort order.
- `schemas/sequon-record-1.schema.json`, `schemas/import-report-1.schema.json` -
  machine-readable structure.
- `tests/digest-vectors.json` - digest vectors for `sequon-digest/1`.
