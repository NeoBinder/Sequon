# Annotation-only operation contract

Status: accepted for R11 implementation, 2026-10-30. <Sequon> owns the pure
canonical transform. <NeoAPI-rs> owns identity, concurrency, persistence and
the public envelope. This contract never edits nucleotide bases.

## 1. Logical library operation

The logical <Sequon> entry point accepts a validated base Record and an ordered
list of annotation operations. It returns an AnnotationOutcome containing:

- a new canonical Record;
- a feature identity map between base/result versions and add operations;
- producer provenance needed by the consumer.

The transform is deterministic and has no database, HTTP, clock, randomness or
other side effect. Before returning it performs Record::validate() and
sequon_algo::verify_digests(). Only neoapi-sequence invokes it. Other NeoAPI
crates receive platform DTOs or canonical JSON and never import <Sequon> types.

The base Record remains unchanged. sequence, length, topology,
digests.sequence, digests.canonical, extras and original source lineage are
preserved exactly. Only features and producer provenance may change, so the new
canonical serialization and payload_sha256 have a new version identity.

## 1.1 Implemented Sequon API (SEQ-035)

The pure sequon-algo surface now implements this contract without a parser, HTTP,
database, clock, randomness or platform identity dependency:

| Function | Result |
| --- | --- |
| apply_annotation_operations(&Record, &[AnnotationOperation]) | AnnotationOutcome: the new validated and digest-verified Record, the FeatureIdentityMap and AnnotationProvenance |
| add_feature(&Record, FeatureInput) | One-operation add form of the same transform |
| update_feature(&Record, usize, FeatureInput) | One-operation in-place replacement at a base source-order index |
| delete_feature(&Record, usize) | One-operation removal and contiguous reindexing |
| save_selection_as_cds(&Record, SaveAsCdsRequest) | One-operation CDS span creation from a triplet selection |

FeatureInput carries only key, so_term, name, location and qualifiers, so no
sequence, envelope, provenance, display colour, template identity or database id
can be supplied through an operation. AnnotationOutcome.provenance records
producer sequon-algo at the exact crate version and algorithm
sequon-annotation/1; the edited Record's provenance.producer becomes
{kind: algorithm, name: sequon-algo, version: <crate version>} while
source_format, source_digest, source_bytes, export_version and extras stay
unchanged.

FeatureIdentityMap contains base_feature_count, result_feature_count,
base_to_result (one Option<usize> per base feature in base source order, null
for a deleted feature) and operation_to_result (operation_index/result_index for
every add and save operation in request order). It deliberately contains no
database feature id.

Platform identity is outside the pure outcome. expected_base_version, the
parent and result payload_sha256, the Idempotency-Key and the persistence
timestamp belong to <NeoAPI-rs> (sections 4, 5 and 6): only the platform resolves
and rechecks an immutable version, and only sequon-io owns canonical JSON
serialization, so sequon-algo must not re-implement it. The consumer computes
the result canonical bytes and payload_sha256 with the Sequon IO serializer and
fills those platform fields into its response envelope.

R11 limits are exported as constants and fail with ANNOTATION_LIMIT_EXCEEDED:
MAX_ANNOTATION_OPERATIONS = 100, MAX_FEATURES_PER_RECORD = 100000,
MAX_SEGMENTS_PER_FEATURE = 1024, MAX_QUALIFIERS_PER_FEATURE = 256 and
MAX_QUALIFIER_VALUE_BYTES = 65536. The contract requires feature and qualifier
keys to be non-empty and bounded without fixing a number; the reviewed R11
bounds are MAX_FEATURE_KEY_BYTES = 255 and MAX_QUALIFIER_KEY_BYTES = 255, with
control characters rejected in qualifier keys. Feature/segment/qualifier/value
limits apply to features supplied by an operation, and the feature-count limit
applies to the result record; pre-existing base features are not retroactively
rejected for a limit they were imported under.

save-selection-as-CDS writes one span (an unwrapped origin-crossing span stays
one segment), key CDS and so_term SO:0000316, the exact optional name, the
caller's ordered qualifiers first and then exactly one /codon_start and one
/transl_table (defaults 1 and 1). Caller-supplied /codon_start, /transl_table or
/translation is ANNOTATION_QUALIFIER_UNSUPPORTED; so is any add or save that
carries /translation. An update may keep /translation only when its ordered
/translation entries equal the base feature's entries exactly.

Typed error codes: RECORD_INVALID and DIGEST_MISMATCH (base or result
validation), ANNOTATION_IDENTITY_CONFLICT (unresolvable index or duplicate
update/delete target), ANNOTATION_FEATURE_INVALID (feature field, location or
selection), ANNOTATION_CDS_LENGTH_INVALID (non-triplet save selection),
ANNOTATION_QUALIFIER_UNSUPPORTED (protected or duplicated protected qualifier),
ANNOTATION_LIMIT_EXCEEDED (an R11 limit) and an empty operation list as
ANNOTATION_FEATURE_INVALID. Messages never contain sequence content or raw
qualifier values.

There is no nucleotide insert, delete, replace or rebase function on this or any
other sequon-algo public surface.

## 2. Operation request

The platform operation envelope is neoapi.annotation-operations/1:

| Field | Rule |
| --- | --- |
| expected_base_version | Required positive integer; all feature_index values refer to this immutable version |
| operations | Ordered array of 1..100 operations; the entire list is atomic |

The public route is contract only, implemented after the <Sequon> revision gate:

POST /sequences/{record_id}/versions

The request also requires the platform Idempotency-Key header. It does not
accept a complete replacement Record, sequence, length, topology, digest,
provenance, extras, display color, template id or database feature id. Any such
field is ANNOTATION_FIELD_FORBIDDEN.

All operations in one request use base-version source-order identity, never an
intermediate index produced by another operation in the same request. A base
feature may be targeted by at most one update or delete. Conflicting duplicate
targets are ANNOTATION_IDENTITY_CONFLICT.

### 2.1 add_feature

{op:add_feature, feature:FeatureInput}

FeatureInput contains only key, so_term, name, location and qualifiers. The new
feature is appended in operation order after all surviving base features. No
name, SO term or feature key is guessed when the caller supplies none.

### 2.2 update_feature

{op:update_feature, feature_index:N, feature:FeatureInput}

feature_index is the base Record's zero-based source-order index. The complete
canonical feature at that position is replaced by FeatureInput; the position of
other surviving features is unchanged. This is not a patch that can smuggle in
sequence or envelope fields.

### 2.3 delete_feature

{op:delete_feature, feature_index:N}

Delete exactly the identified base feature. Other features retain their relative
order and are reindexed contiguously in the result. No coordinates are rebased.

### 2.4 save_as_cds

{op:save_as_cds, selection:{start,end}, strand:1 or -1,
name:optional string or null, codon_start:optional 1, 2 or 3,
transl_table:optional 1 or 11, qualifiers:optional ordered qualifier array}

The selection follows the canonical linear/circular positive-length rules and
becomes one span, including an unwrapped circular origin-crossing span. strand
0, zero length, multiple wraps and out-of-range bounds are invalid. R11
save-as-CDS requires selection length divisible by three and has no partial
boundary flags; otherwise it returns ANNOTATION_CDS_LENGTH_INVALID. This is not
ORF discovery: start/stop presence is not invented or required.

The new feature has key CDS, so_term SO:0000316 and the exact optional name.
Missing codon_start/transl_table default to 1. The transform writes each of
those two qualifiers exactly once and rejects caller-supplied duplicates. A
caller-supplied /translation is ANNOTATION_QUALIFIER_UNSUPPORTED because the
computed translation belongs to derived analysis. Other ordered qualifiers,
including duplicates where biologically meaningful, are preserved after key and
value validation. The new CDS is appended like add_feature.

## 3. Feature and location validation

FeatureInput follows the canonical model, not a UI projection:

- key is non-empty and bounded; unknown keys remain verbatim and so_term may be
  null rather than guessed;
- name is nullable and never synthesized from product/gene;
- location operator is span, join or order with canonical segment arity and
  source order preserved;
- every segment obeys linear/circular bounds and at most one circular wrap;
- zero-length between-position sites remain distinct from empty selections and
  never wrap;
- strand is -1, 0 or 1; a CDS requires -1 or 1;
- start_partial/end_partial are preserved exactly on add/update and never used
  to widen coordinates;
- qualifiers remain an ordered key/value array, with null allowed for bare
  qualifiers. Keys are non-empty, bounded and control-character free; values
  are bounded strings or null.

add_feature rejects a caller-supplied /translation. update_feature permits
/translation only when its ordered qualifier entries are identical to those in
the base feature; it cannot introduce, remove or change that source metadata.
save_as_cds rejects it as stated above. Derived analysis never uses any stored
/translation as its answer. Platform display color, user feature-template
identity and database ids are never written into canonical features or
qualifiers.

R11 limits are 100 operations per request, 100,000 features per result, 1,024
segments per feature, 256 qualifiers per feature and 64 KiB per qualifier
value. Exceeding one is ANNOTATION_LIMIT_EXCEEDED; nothing is truncated.

## 4. Atomic ordering and feature identity map

The pure transform applies these rules in one logical step:

1. Resolve every update/delete feature_index against expected_base_version and
   reject duplicate targets before changing anything.
2. Apply each update at its base position and remove each deleted base feature.
3. Preserve the relative order of every surviving feature.
4. Append add_feature and save_as_cds results in their request order.
5. Reindex the result contiguously from zero in canonical source order.

AnnotationOutcome.feature_identity_map contains:

- base_to_result entries for every surviving base feature, including null for a
  deleted result index;
- operation_to_result entries for every add/save operation;
- the expected base version, result payload_sha256 and result feature count.

Canonical features still contain no stable database id. Identity is unique only
within one immutable version. Platform style/template metadata uses this map
when attaching to the new version; it never rewrites an old version.

Any invalid operation rejects the entire list. There is no partial transform,
partial database append or silent skip.

## 5. Provenance and import-report boundary

The new Record retains the base provenance.source_format, source_digest,
source_bytes and export_version as original source lineage. Its
provenance.producer becomes {kind:algorithm, name:sequon-algo,
version:<exact crate version>}. This identifies the producer of the edited
Record; it does not claim that the original source bytes alone encode the edit.

The platform version/audit record separately persists expected_base_version,
parent payload_sha256, the ordered operations or their canonical request hash,
Idempotency-Key, authenticated owner, persistence timestamp, <Sequon> revision,
algorithm version and result payload_sha256. These platform fields are not
smuggled into the canonical envelope.

A new annotation version does not inherit the old import report, parser
warnings, dropped entries or preserved blocks. The original report remains
attached only to its original import version. The annotation version has no
new sequon/import-report/1 unless a real new import occurs; an empty platform
report field must not be presented as a parser report.

## 6. Public response and persistence contract

A successful POST returns 201 with SequenceAnnotationVersionResponse:

| Field | Content |
| --- | --- |
| schema | neoapi.annotation-version/1 |
| document | The complete SequenceDocument for the newly appended immutable version |
| base_version | The expected base version actually transformed |
| feature_identity_map | The base/result and operation/result mapping defined above |
| producer | Algorithm name/version and approved <Sequon> revision |

The pure transform completes before the database transaction. PostgreSQL then
rechecks ownership, expected_base_version and idempotency in one transaction,
appends the immutable version/projections/audit record and advances current.
Any failure aborts the entire append. A concurrent current-version change
returns 409 sequence_version_conflict and never overwrites the other version.

Replaying the same owner + record + Idempotency-Key + canonical request returns
the original result without another version. Reusing that key with a different
request returns 409 idempotency_conflict. SEQ-160/170 owns the detailed
transaction, audit and migration implementation; this paragraph fixes the
scientific version boundary they must preserve.

## 7. Capability and typed errors

Annotation writing uses capability sequence.annotation.write, independent of
document/analysis capability. It is advertised only after the exact <Sequon>
revision and NeoAPI write path pass qualification. Unknown returns 503
capability_unknown; stale, missing or unqualified capability returns 503
capability_unavailable; an absent operation returns 501 not_implemented.

| Condition | HTTP/public code |
| --- | --- |
| Missing/foreign record or base version | 404 not_found |
| Base-version race | 409 sequence_version_conflict |
| Idempotency-key reuse with different request | 409 idempotency_conflict |
| Forbidden envelope/base field | 422 annotation_field_forbidden |
| Duplicate target or unresolvable feature identity | 422 annotation_identity_conflict |
| Invalid feature/location/selection | 422 annotation_feature_invalid |
| Non-triplet save-as-CDS selection length | 422 annotation_cds_length_invalid |
| Unsupported or duplicate protected qualifier | 422 annotation_qualifier_unsupported |
| Operation/feature/segment/qualifier limit | 413 annotation_limit_exceeded |
| New Record validation or digest failure | 500 sequence_payload_corrupt |
| Capability unknown/unavailable | 503 capability_unknown / capability_unavailable |

Errors never include sequence content or raw qualifier values. No failed edit
is presented as a new server version.

## 8. Explicit non-goals and fixtures

This contract has no nucleotide insert/delete/replace, annotation rebasing,
SnapGene .dna write-back, primer design, ORF discovery, global six-frame view,
cloning/digest simulation, protein record/IO, whole-JSON overwrite, version
restore/compare, undo/redo, <NeoFlow> operation or async job.

Synthetic R11 edit fixtures verify feature ordering, source-order identity,
origin-crossing spans, save-as-CDS and unchanged molecule digests. Real
GenBank/SnapGene fixtures verify that imported feature/location/qualifier data
survives annotation transforms without silent loss. The two fixture classes
remain named and separate; unsupported input always produces a typed error or
explicit preserved/dropped provenance, never a guessed edit.
