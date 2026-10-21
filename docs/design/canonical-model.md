# Canonical model

**Status: draft, not accepted.** Nothing in this document is a decision yet.
This is the first milestone of the repository: until it is accepted, no adapter
or algorithm is written against an assumed shape.

## Why this document exists

Every format we read (GenBank, FASTA, SnapGene `.dna`) and every consumer we
serve (storage, matching, visualisation) has to agree on one representation.
Formats are adapters; the model is the truth. Getting this wrong is expensive to
undo, so it is decided in writing first.

## What the model must express

1. One sequence — DNA or protein — with molecule type and topology (linear,
   circular).
2. Features with type, name, location, strand, and qualifiers. Locations may be
   a single span, joined spans, ordered spans, and may cross the origin of a
   circular sequence.
3. Provenance: source format, parser name and version, digest of the source
   bytes, and any warnings raised while reading.
4. Identity: a digest of the normalised sequence, plus a rotation- and
   strand-invariant digest for circular records, so the same molecule exported
   differently is recognised as the same molecule.
5. Immutability: a record version describes exactly one sequence; editing
   produces a new version.
6. Extensions: blocks a format carries that the standard subset cannot hold are
   preserved explicitly, not dropped.
7. Round-trip: what the model can express must come back out unchanged;
   everything else must be reported.

## Proposed envelope

```json
{
  "schema": "sequon/record/1",
  "kind": "dna",
  "molecule_type": "ds-DNA",
  "topology": "circular",
  "name": "pET28a-SUMO-hGLP1",
  "description": null,
  "length": 6142,
  "sequence": "ATG...",
  "digests": {
    "sequence": "sha256:...",
    "canonical": "sha256:..."
  },
  "features": [
    {
      "type": "CDS",
      "so_term": "SO:0000316",
      "name": "hGLP1(7-37)",
      "location": {
        "kind": "span",
        "start": 731,
        "end": 1861,
        "strand": 1
      },
      "qualifiers": {
        "translation": "HAEGTFTSDV..."
      }
    }
  ],
  "provenance": {
    "source_format": "snapgene-dna",
    "source_digest": "sha256:...",
    "parser": "sequon-io",
    "parser_version": "0.1.0",
    "warnings": []
  },
  "extras": {}
}
```

## Tentative field rules

| Field | Type | Proposal | Reason |
| --- | --- | --- | --- |
| `schema` | string | Fixed identifier per version, e.g. `sequon/record/1` | Documents are self-describing; no extension-based versioning |
| `kind` | enum | `dna` \| `protein` | Drives validation and which algorithms are legal |
| `topology` | enum | `linear` \| `circular` | Stored explicitly: GenBank only sets `circular` for full-length records |
| `sequence` | string | Uppercase, no whitespace, IUPAC letters | One normal form; the digest is computed over it |
| `length` | integer | Redundant with `sequence` | Cheap integrity check when streaming |
| `features[].location` | object | 0-based, half-open, `strand` in `-1,0,1` | Internal maths; conversion happens in adapters, once |
| `features[].qualifiers` | object | Free-form string map, keys preserved | Filtering qualifiers makes round-trip lossy |
| `provenance.warnings` | array | Structured: code, message, source position | Reports what was dropped or reinterpreted |
| `extras` | object | Format-specific payloads, namespaced | Keeps unrepresentable blocks instead of discarding them |

## Open questions

These must be closed before code:

1. **Coordinate basis.** Keep 0-based half-open internally and convert at the
   adapter boundary, or mirror INSDC 1-based closed to reduce conversion risk?
2. **Near-origin features.** Clamp spans to the sequence length and split into
   segments, or allow `end > length` and let consumers wrap? How does that
   interact with joined locations?
3. **Qualifier typing.** Free-form map only, or a typed set for a few well-known
   keys (`translation`, `product`, `note`, `regulatory_class`) with a
   free-form remainder?
4. **Protein features.** Express domains and sites with INSDC keys, or introduce
   a protein-specific vocabulary? How much does the DNA and protein shape need
   to share?
5. **Sequence payload.** Inline the sequence in every document, or allow a
   document that carries only digests plus an external reference for very large
   sequences?
6. **Document granularity.** One record per file, or a bundle document that can
   hold several records (a construct plus its parents)?
7. **Identifiers.** Caller-supplied stable ids, ULIDs, or none in the document
   (identity is a storage concern)?
8. **Determinism.** Fixed field order and array order in serialized output:
   sorted by coordinates, or preserve input order?
9. **Extension policy.** Who may write to `extras`, and how do we prevent two
   adapters from fighting over the same namespace?
10. **Validation strictness.** Reject non-IUPAC characters, unknown feature
    types, or out-of-range coordinates — or accept with warnings? Which failures
    are errors?

## Out of scope for the first version

- SBOL-style design hierarchy.
- Evidence or confidence on annotations.
- Binary encoding or compression framing.
- Storing records, permissions, or versions; those belong to the consumer.

## Decision log

| Date | Decision | Rationale |
| --- | --- | --- |
| — | Model not decided | |
