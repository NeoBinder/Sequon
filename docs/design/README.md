# Design documents

Accepted documents are contracts: they describe behavior we commit to and are
changed together with the code and schemas they govern. Drafts are proposals
under review; they may change without notice.

| Document | Status | Scope |
| --- | --- | --- |
| [canonical-model.md](canonical-model.md) | accepted, M0 (2026-10-21); versioning section revised 2026-10-22; revised 2026-10-23 for circular cross-origin coordinates | Semantic contract for `sequon/record/1` and `sequon/import-report/1`: fields, required sources, locations, qualifiers, digests, provenance, extras, report semantics, versioning |
| [canonical-json-profile.md](canonical-json-profile.md) | accepted, M0 | Byte-level serialization: encoding, field order, nullable fields, numbers, escaping, pretty-printing, diagnostic ordering, determinism promises |
| [diagnostics.md](diagnostics.md) | accepted, M0 | Diagnostic code table (`codes_version 2026-10-21/1`), severities, source positions, ordering, dropped/preserved vocabulary, unknown-code behavior |
| [genbank-adapter.md](genbank-adapter.md) | accepted, finalized at M3 (2026-10-21) | GenBank reader and writer mapping: boundaries, header, FEATURES, location grammar, ORIGIN, multi-record, `WriteOutcome`, SO table, worked example, fixture inventory |
| [snapgene-dna.md](snapgene-dna.md) | accepted, finalized at M4 (2026-10-21) | SnapGene `.dna` container, packet matrix, 2-bit encoding hypothesis, XML mapping, history/traces, codecs (Q9), malicious input, unverified-fact register, fixture inventory |
| [consumer-integration.md](consumer-integration.md) | accepted, M5 (2026-10-21); revised 2026-10-22 for P1 projection, pin policy, compatibility window and embedded schema bytes; revised 2026-10-23 for circular cross-origin coordinates | Platform pinning, payload/transport ownership, report consumption, P1 projection and gaps; example in `crates/sequon-io/examples/` |
| [testing.md](testing.md) | accepted, M5 | Test layers, fixtures and goldens, CI matrix, fuzz targets, dependency gate and performance regression |

## Rules

- The canonical model is the semantic source of truth. `schemas/` is its
  machine-readable copy; an accepted document and its schema change in the same
  commit, and a disagreement is a bug.
- Diagnostic codes are append-only within a `codes_version`; a new code bumps
  the version and never changes the meaning of an existing code.
- Drafts record format facts and open questions. A fact that is not confirmed by
  a fixture is marked as a hypothesis, not stated as fact.
- Start with the [repository README](../../README.md), or the canonical model
  when you need the contract itself.
