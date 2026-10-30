# Canonical document read contract

Status: accepted for the R11 implementation, 2026-10-30. This is a consumer
contract for <NeoAPI-rs>; it does not add HTTP, persistence or jobs to <Sequon>.
The canonical payload semantics remain owned by [canonical-model.md](canonical-model.md),
and the consumer duties in [consumer-integration.md](consumer-integration.md)
continue to apply.

## 1. Purpose and identity

A document read returns one complete sequon/record/1 Record at one immutable
platform version. It is the only R11 read path for DNA bases and canonical
features. The existing record-detail projection remains available for list and
summary UI, but it is not a substitute for this document.

<Sequon> owns Record, its schema, coordinates and digest algorithms. The
platform owns record_id, version numbers, ownership, persistence and the
transport envelope. Neither layer may invent a second sequence representation.

A document is identified by all of:

- record_id: platform UUID;
- version: positive integer, immutable within that record;
- payload_sha256: SHA-256 of the exact canonical JSON bytes accepted at
  persistence, encoded as 64 lowercase hexadecimal characters without a prefix;
- schema $id and schema_sha256: the embedded schema identity pins;
- digest_algorithm and the Record's two digest values;
- producer: the canonical producer stored in the Record.

The molecule digest and the payload digest have different meanings. An
annotation-only edit preserves digests.sequence and digests.canonical but
changes version and payload_sha256.

## 2. Public routes (contract only)

These paths live in the existing public v2 OpenAPI document. Their implementation
is deferred to SEQ-020; this note does not advertise them as shipped.

| Operation | Route | Behavior |
| --- | --- | --- |
| getSequenceDocument | GET /sequences/{record_id}/document | Resolve the caller-owned current version, then return that immutable document |
| getSequenceVersionDocument | GET /sequences/{record_id}/versions/{version}/document | Return exactly the requested immutable version |

record_id is a UUID and version is an integer of at least 1. A missing record,
missing version or record not owned by the caller returns the same 404
not_found response; ownership is never disclosed through a different payload.
The current route performs one authoritative current-version resolution and
then applies the same validation and response mapping as the versioned route.

No route accepts a replacement Record on GET. No client-side GenBank detour is
permitted. Export remains the existing format-specific operation and is not a
document-read shortcut.

## 3. SequenceDocument DTO

The response Content-Type is application/json. The envelope schema identifier is
neoapi.sequence-document/1, versioned independently of the Record schema.

| Field | Type | Rule |
| --- | --- | --- |
| schema | string | Exactly neoapi.sequence-document/1 |
| record_id | UUID | Platform record identity |
| version | integer >= 1 | The resolved or requested immutable version |
| payload | object | One complete sequon/record/1 Record, including sequence, features, digests, provenance and extras |
| payload_sha256 | 64 lowercase hex | Pin of the accepted canonical bytes, not a hash recomputed by the browser |
| schema_urn | string | For v1, urn:sequon:schema:record:1 |
| schema_sha256 | 64 lowercase hex | SHA-256 of the exact embedded schema bytes |
| digest_algorithm | string | For v1, sequon-digest/1 |
| producer | object | Copy of payload.provenance.producer; no fabricated actor or timestamp |
| created_at | RFC 3339 timestamp | Platform persistence time for this version |

The payload is a JSON object, not a double-encoded string and not a GenBank or
UI projection. Transport JSON is not a new canonical byte serialization:
whitespace or object-member order on the wire is not a payload identity.
payload_sha256 remains the pin of the canonical bytes produced and verified by
the Rust consumer at persistence. Clients must not JSON.stringify the response
and call the result canonical bytes or expect that string to hash to
payload_sha256.

The response adds no display color, feature-template value, database feature id,
analysis result or import diagnostic to payload. Those are separate platform
metadata or contracts.

## 4. Read validation and compatibility

neoapi-sequence performs the complete accept step; handlers do not inspect
scientific fields. After loading stored payload data it must:

1. parse with the strict <Sequon> canonical JSON reader;
2. call Record::validate();
3. call sequon_algo::verify_digests();
4. verify the stored record/version identity, schema pins, producer,
   payload_sha256, topology, length and digest projections agree with the
   Record and persistence row;
5. apply the semver compatibility window before returning the document.

A structural or digest mismatch is sequence_payload_corrupt (500), not a caller
validation error and not something to repair silently. A producer/schema
outside the supported compatibility window is sequence_payload_incompatible
(503). Neither response includes sequence content. Startup is not refused by a
stored pin mismatch, but the affected document is not served as verified.

Only neoapi-sequence may perform these <Sequon> calls. neoapi-api,
neoapi-db and neoapi-contracts use platform DTOs, JSON values or bytes and never
import a <Sequon> Rust type.

## 5. Cache and freshness

HTTP responses use Cache-Control: private, no-store; shared HTTP caches are not
part of the contract. A client service may keep a private in-memory cache only
with the complete key:

(record_id, version, payload_sha256, schema_urn, schema_sha256,
digest_algorithm, producer version, compatibility window).

A current-document cache entry also carries the resolved version and is
invalidated after a successful new-version write or any observed current
change. A versioned document is immutable and never overwritten. An aborted,
foreign, corrupt or error response never replaces a previously valid entry.
Credentials, ownership and sequence content are never cache keys or log values.

## 6. Capability and typed errors

Document reading has a capability independent of analysis and annotation
writing. It is advertised only when the pinned consumer path is implemented and
qualified. Unknown capability yields 503 capability_unknown; missing, stale or
unqualified capability yields 503 capability_unavailable. Analysis or edit
unavailability does not disable a qualified document read.

| HTTP | Public code | Meaning |
| --- | --- | --- |
| 401/403 | existing authentication errors | Identity or session requirements failed |
| 404 | not_found | Missing, foreign or unknown version, with no ownership disclosure |
| 500 | sequence_payload_corrupt | Stored payload, projection or digest validation failed |
| 503 | sequence_payload_incompatible | Stored producer/schema is outside the supported reader window |
| 503 | capability_unknown / capability_unavailable | Capability cannot be proven available |

## 7. Consumer responsibilities

NeoHub obtains document data only through src/api/client.ts and
SequenceService. It may format 0-based half-open coordinates for display and
manage selection/layout state, but it does not reserialize canonical JSON,
recompute digests, parse GenBank, or synthesize missing DNA/features. A failed
or foreign read shows an honest error/empty state and never displays stale data
as though it belonged to the requested version.
