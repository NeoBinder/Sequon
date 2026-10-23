# SnapGene .dna adapter design

Status: **M0 draft** (2026-10-21), updated with lead decisions and calibrated against a real
SnapGene corpus on the same date. Owner: `snapgene`. Feature: `sequon-io/snapgene`,
import-only.
Related: [canonical model](canonical-model.md) (accepted), development plan section 6,
[diagnostics](diagnostics.md), [GenBank adapter](genbank-adapter.md), `testdata/README.md`.

This document is a draft for M0 review. It records the container layout, the packet matrix,
the XML mapping, the verified codec selection (Q9) and the fixture plan. It does **not**
describe shipped code: no adapter, parser, model type or fixture exists in this milestone.

Every statement about the byte layout is tagged:

- **H** - hypothesis. Derived from reference projects and format notes, not yet confirmed
  against a fixture produced by SnapGene. The implementation must not rely on it until the
  named fixture confirms it.
- **R** - reader rule. Our own defensive behaviour, independent of the format.
- **V** - verified. Confirmed against the local corpus below, or against the crate
  registry/toolchain, with the evidence recorded in section 2, section 13 and
  `.agents/task/plasmid-sequence-design/snapgene-corpus-findings.md`.

The M4 calibration corpus is a real SnapGene export of pET-28a(+), supplied by the repository
owner and kept only in the gitignored `testdata/corpus-local/`:

| Field | Value |
| --- | --- |
| Bytes | 27646 |
| sha256 | `f298f4c87de87d193597771d63f515cb7906f8d71efb0ebc3d920e1a188f6a0d` |
| Checked | 2026-10-21, read-only, standard-library analysis |

It confirms the cookie, the type 0 DNA layout, seven packet types, the feature/notes/properties
shapes and the coordinate convention. Facts the corpus does not exercise remain **H** and keep
their fixture requirement. The corpus is not a fixture and is never committed or copied into
`testdata/snapgene/`.

No hypothesis in this document is a promise about the format. Section 10 is the register of
format facts and their verification status; a fact leaves the unverified part only when the
named evidence confirms it and this document records the measurement.

## 1. Scope and legal boundary

### 1.1 Scope

- The adapter reads SnapGene `.dna` bytes into `sequon/record/1` plus
  `sequon/import-report/1`. One `.dna` file produces exactly one record (D10 multi-record
  policy is a GenBank concern; the container has no record list).
- It lives behind the `snapgene` feature of `sequon-io` (D13). With the feature off,
  `sequon-io` compiles and tests without `lzma-rs` or an XML tokenizer in its tree.
- It is import-only (D14). There is no `write_snapgene`, no SnapGene writer type, no
  serialization trait implementation and no binary output path. Absence at the type level is
  deliberate: a future writer must not be reachable by adding a flag.
- It never becomes a canonical representation. The record is the canonical form; SnapGene
  blocks the model cannot express go to `extras.snapgene` or to the import report.
- Supported input is a single complete in-memory byte slice, consistent with S9. Streaming is
  out of scope for v1.
- Output is deterministic: two imports of the same bytes with the same crate version produce
  an identical record and an identical diagnostic sequence (section 9).

### 1.2 Non-goals

- Writing or editing `.dna`, including "preserve unknown packets then write them back".
- Mapping the history tree into a family tree, lineage or `sequence_relations`. The block is
  validated and preserved, never interpreted (D14).
- Reconstructing SnapGene's construction simulation, primer design or alignment semantics.
- RNA/protein records. The cookie sequence type selects `kind`; v1 rejects anything that is
  not DNA with a typed error (D15). It never downgrades to DNA.
- Treating SnapGene as a second storage model, or exposing `extras.snapgene` as a public API
  that consumers are expected to program against.

### 1.3 Legal boundary

- `.dna` is a reverse-engineered binary container with no public specification. This adapter
  is an independent implementation written from observed file behaviour and public
  reverse-engineering notes. It does not copy, vendor or link any third-party SnapGene
  parser, and it does not copy GPL-licensed source. Reference projects are read for format
  knowledge only, exactly as `Sequon/AGENTS.md` requires.
- The adapter processes files supplied by the user. It does not download, embed or
  redistribute sample files. SnapGene-exported fixtures are not committed; see Q8 and
  `testdata/README.md`. In-repo fixtures are synthetic files built by this project.
- The general-purpose codecs named in section 2 (`lzma-rs`, `quick-xml`) are MIT-licensed and
  unrelated to SnapGene. They implement compression and XML syntax, not the `.dna` format.
- Write support requires a legal review before any design or code exists, because writing
  would require reproducing container details we currently only need to read. Until that
  review accepts writing, D14 stands and this document must not be read as a step toward a
  writer.
- Reverse-engineering and interoperability rules differ by jurisdiction. Legal review before
  the first public release is owned by `lead`, not by this adapter.

## 2. Codec verification and Q9 decision (M0 gate)

### 2.1 Method

Checked on 2026-10-21 with the toolchain and registry available on the build host:

- `cargo 1.97.1` / `rustc 1.97.1`.
- The cargo registry is a sparse mirror of crates.io configured in the host cargo config.
- `cargo info <crate> --registry crates-io` reads the registry metadata.
- Exact release timestamps and declared `rust-version` values were read from the sparse index
  entry for each crate.
- The resolved dependency tree and each crate's `Cargo.toml` and license files were inspected
  in the local cargo registry cache after `cargo fetch` in a throwaway probe project.
- The crates.io JSON API was not reachable from this host, so all facts below come from the
  registry index and the downloaded crate metadata, which are the same sources `cargo`
  itself uses.

The probe project lived outside the repository. It was deleted after the check and added
nothing to this repository.

### 2.2 lzma-rs

| Item | Value | Evidence |
| --- | --- | --- |
| Latest 0.3.x | **0.3.0** | `cargo info lzma-rs --registry crates-io` |
| Released | 2023-01-04T00:34:47Z | sparse index `pubtime` for 0.3.0 |
| License | **MIT** | registry metadata and `LICENSE` in the crate |
| Edition | 2018 | crate `Cargo.toml` |
| Declared MSRV | none (`rust-version` absent) | crate `Cargo.toml`; treat as unbounded, verify in CI |
| Pure Rust | yes, `#![forbid(unsafe_code)]`, no `build.rs`, no C or system library | `src/lib.rs`, crate root |
| Features used | none; default feature set is empty | crate `Cargo.toml` `[features]` |
| Available features (unused) | `enable_logging`, `raw_decoder`, `stream` | registry metadata |
| Transitive dependencies | `byteorder 1.5.0` (Unlicense OR MIT), `crc 3.4.0` (MIT OR Apache-2.0) -> `crc-catalog 2.5.0` (MIT OR Apache-2.0) | `cargo tree`, crate manifests |

API facts relevant to the reader:

- `lzma_rs::lzma_decompress_with_options<R: BufRead, W: Write>(input, output, &Options)`
  streams decoded bytes into an arbitrary `Write` sink. It does not need the whole output in
  memory.
- `decompress::Options` has `unpacked_size` (default `ReadFromHeader`), `memlimit`
  (default unlimited) and `allow_incomplete` (default false). `memlimit` bounds the decoder
  dictionary buffer; the buffer grows lazily and returns `LzmaError` when the next index
  would exceed it.
- The LZMA-alone header carries a 64-bit little-endian unpacked size. The decoder treats
  `0xFFFF_FFFF_FFFF_FFFF` as "end marker present" and otherwise reads the value. The declared
  size is never an allocation request and is not a trusted limit: when it is present the
  decoder stops after that many output bytes, so `CappedWriter` remains the only authority on
  how much output a block may produce.
- Writing failures reach us as `lzma_rs::error::Error::IoError`, so a capped writer can abort
  decompression by returning an `io::Error`; the reader sets a sentinel on its own writer to
  distinguish a limit hit from an input failure.
- A truncated stream surfaces as `Error::IoError` (for example `UnexpectedEof`, "failed to
  fill whole buffer") rather than `LzmaError`; both are failure results, never a silent short
  decode. The reader maps the non-sentinel cases to `DNA_LZMA_MALFORMED`.
- `allow_incomplete` only affects the optional `stream` API, which this adapter does not
  enable. The non-stream entry point always runs its `Finish` processing mode, which
  validates end-of-stream or declared-size completion.

### 2.3 quick-xml

| Item | Value | Evidence |
| --- | --- | --- |
| Current version | **0.42.0** | `cargo info quick-xml --registry crates-io` |
| Released | 2026-08-22T19:22:01Z | sparse index `pubtime` for 0.42.0 |
| License | **MIT** | registry metadata and `LICENSE-MIT.md` |
| Edition | 2024 | crate `Cargo.toml` |
| Declared MSRV | **1.86** | `rust-version = "1.86"` in the crate manifest and the index entry |
| Default features | empty (`default = []`) | registry metadata |
| Features used | none | pinning plan in section 2.7 |
| Transitive dependencies | `memchr 2.8.3` (Unlicense OR MIT) | `cargo tree`, crate manifest |
| DTD support | none: DTD is surfaced as a `DocType` event, never parsed into an entity table | `src/events/mod.rs`, `src/reader/mod.rs` |
| External entity loading | none: the crate performs no I/O beyond the input slice; there is no resolver that can fetch a URL | crate source review |
| Entity expansion | opt-in per text value via `escape::unescape` / `unescape_with`; the reader itself only emits `GeneralRef` events | `src/escape.rs` |
| Byte offsets | yes: `Reader::buffer_position() -> u64` (end of the last emitted event) and `Reader::error_position() -> u64` (start of the markup element that caused the last error) | `src/reader/mod.rs` |
| Input model | `Reader::from_str` over a `&str` gives a zero-copy slice reader with `read_event()` | `src/reader/slice_reader.rs` |

Configuration surface in 0.42.0 (`reader::Config`): `allow_dangling_amp` (default false),
`allow_unmatched_ends` (false), `check_comments` (false), `check_end_names` (true),
`expand_empty_elements` (false), `trim_markup_names_in_closing_tags` (true),
`trim_text_start` / `trim_text_end` (false). None of these controls DTD or entity
resolution, because the reader has no such machinery; safety comes from refusing the events
(section 2.8).

### 2.4 DTD, entity and offset evidence

A throwaway probe compiled against exactly `lzma-rs 0.3.0` and `quick-xml 0.42.0` produced
this output (abridged):

```text
-- dtd --
REJECT: DTD at end-offset 54
-- offsets-and-entities --
start root at end-offset 6
start Feature at end-offset 24
empty Segment at end-offset 47
start Note at end-offset 63
text "a" end-offset 64
entity-ref "amp" end-offset 69
text "b " end-offset 71
entity-ref "lt" end-offset 75
text "x" end-offset 76
entity-ref "gt" end-offset 80
-- malformed --
start root at end-offset 6
start a at end-offset 9
malformed xml: ill-formed document: expected `</a>`, but `</b>` was found error_position=9
```

Conclusions:

- A `<!DOCTYPE ...>` declaration arrives as a single `Event::DocType`; rejecting that event
  is sufficient to refuse DTDs. No entity definitions are installed by the tokenizer.
- Named and numeric references arrive as separate `Event::GeneralRef` events. The reader can
  accept exactly the five predefined XML entities and numeric character references, and
  reject everything else. `BytesRef::resolve_char_ref()` decodes numeric references only.
- `buffer_position()` gives a usable end offset per event, and `error_position()` points at
  the start of the offending markup, which satisfies the "byte offset for errors" rule.
- Malformed markup is a typed `quick_xml::Error`; there is no panic path in the tokenizer.

### 2.5 Decision for Q9

**Q9 decision: use `quick-xml =0.42.0` as a general-purpose XML tokenizer, pinned exactly,
with default features disabled, and reject DTD and non-predefined entity references at the
event level. Do not hand-write a pull parser for v1.**

Rationale:

1. It is actively maintained (release 2026-08-22), MIT-licensed and small: with default
   features off the only transitive dependency is `memchr`.
2. It exposes the two properties the security requirements need: byte offsets
   (`buffer_position`, `error_position`) and an event stream in which DTD and entity
   references are explicit, refusable events rather than implicit expansion.
3. It contains no entity registry, no external resolver and no DTD parser, so the absence of
   a "disable DTD" feature is a property we can enforce structurally: the reader refuses
   `DocType` and only resolves an allowlist of five names plus numeric references.
4. It is a generic XML tokenizer, not a domain parser. All SnapGene element names and
   semantics stay in this repository, which keeps the adapter within the AGENTS.md exception
   for general-purpose codecs.
5. Its cost is a raised toolchain floor (edition 2024, rustc 1.86). Under Q10 ("MSRV = two
   stable minors behind at first release"), a first release on stable 1.97 or later has an
   MSRV above 1.86, so the floor is acceptable. If Q10 ever sets an MSRV below 1.86, pin
   `quick-xml` 0.41.0 (registry `rust-version = "1.79"`) instead; the same API facts above
   were re-checked only for 0.42.0 and must be re-verified for a downgrade.

Rejected alternative: a hand-written pull parser for the SnapGene XML subset. It would remove
the toolchain floor and one dependency, but it would re-introduce the exact classes of defect
this design otherwise avoids (offset bookkeeping, attribute quoting, character references,
CDATA, well-formedness). The SnapGene XML subset is not fixed by a specification; writing a
parser against an unverified subset would make malformed-input handling untestable until
fixtures exist. `quick-xml` is the lower-risk option for M4. A hand-written parser remains
the documented fallback if a security review rejects the dependency; that fallback must then
implement the same event-level rejection rules and offsets.

### 2.6 Transitive dependency licence check

All resolved crates in the snapgene-only tree, with the license declared in the crate
metadata:

| Crate | Version | License | License files present |
| --- | --- | --- | --- |
| `lzma-rs` | 0.3.0 | MIT | `LICENSE` |
| `byteorder` | 1.5.0 | Unlicense OR MIT | `UNLICENSE`, `LICENSE-MIT`, `COPYING` |
| `crc` | 3.4.0 | MIT OR Apache-2.0 | `LICENSE-MIT`, `LICENSE-APACHE` |
| `crc-catalog` | 2.5.0 | MIT OR Apache-2.0 | `LICENSES/` |
| `quick-xml` | 0.42.0 | MIT | `LICENSE-MIT.md` |
| `memchr` | 2.8.3 | Unlicense OR MIT | `UNLICENSE`, `LICENSE-MIT`, `COPYING` |

**Result: no GPL, LGPL, AGPL, MPL, SSPL or other copyleft license appears in the resolved
tree.** Every crate is MIT, MIT/Apache-2.0 dual, or Unlicense/MIT dual, all compatible with
the workspace MIT declaration. This is a metadata inspection, not a legal opinion;
`cargo deny check licenses` remains the CI gate (plan section 7.8). Re-run this check
whenever a pinned version changes.

### 2.7 Cargo feature pinning plan

The workspace `Cargo.toml` and the dependency additions are owned by `lead`; this is the
proposed pinning, recorded here so the design note names the codecs and versions
(AGENTS.md rule). Nothing in this document authorizes adding the dependencies in M0.

```toml
# crates/sequon-io/Cargo.toml
[dependencies]
lzma-rs = { version = "=0.3.0", optional = true, default-features = false }
quick-xml = { version = "=0.42.0", optional = true, default-features = false, features = [] }

[features]
default = ["genbank"]
snapgene = ["dep:lzma-rs", "dep:quick-xml"]
```

Rules:

- Exact `=` versions for both codecs. They are the only crates in the tree whose parser
  behaviour affects record output; a floating minor would let a routine update change
  diagnostics or rejection behaviour without a design-note change.
- `optional = true` plus `dep:` feature entries means the crates are absent from
  `cargo tree -p sequon-io --no-default-features` and from the default `genbank` build.
  `snapgene` never enables the other adapter's dependencies and vice versa.
- No feature of either crate is enabled: `quick-xml` gets no `encoding`, `serialize`,
  `serde`, `escape-html` or `async-tokio`; `lzma-rs` gets no `stream`, `raw_decoder` or
  `enable_logging`. In particular `escape-html` stays off so the accepted entity set stays
  at the five XML entities.
- If `lead` prefers workspace-level dependency pins, the same exact versions move to
  `[workspace.dependencies]`; the feature wiring above stays in `sequon-io`.
- Before the pin lands, re-run the section 2.2/2.3 commands; if a newer patch exists, record
  the new version and its licence/date here first.

### 2.8 Security configuration

**LZMA path.** Call the explicit options entry point, never bare `lzma_decompress`:

```text
Options {
    unpacked_size: UnpackedSize::ReadFromHeader, // default, kept explicit
    memlimit: Some(max_lzma_dict_bytes),         // bounds the decoder dictionary buffer
    allow_incomplete: false,
}
```

- Output goes through `CappedWriter`, which fails the write when the cumulative decoded bytes
  exceed `max_lzma_output_bytes`, or when they exceed
  `max(compressed_block_len, 1) * max_lzma_ratio`. Both are reported as
  `DNA_DECOMPRESSION_LIMIT` (error) with the packet offset.
- `memlimit` is set from `max_lzma_dict_bytes` (default 8 MiB), an **M0 addition to plan
  section 3.6**: the plan's limit table has `max_lzma_output_bytes` and `max_lzma_ratio` but
  no dictionary bound, and without one a crafted 4 GiB dictionary field makes the decoder
  allocate up to that size before the output cap can fire. Lead approved this additive limit
  on 2026-10-21 (O-6).
- If fixture F-20 confirms that the history payload is an LZMA-alone stream (1 property byte,
  4-byte little-endian dictionary size, 8-byte little-endian unpacked size), the reader parses
  that 13-byte header first and rejects a dictionary above `max_lzma_dict_bytes` or a declared
  unpacked size above `max_lzma_output_bytes` with `DNA_DECOMPRESSION_LIMIT` before any
  allocation. `Options.memlimit` repeats the bound inside the decoder. If the payload turns
  out to be raw LZMA without that header, only the writer cap and `memlimit` apply, and a
  memory-limit error is reported as `DNA_LZMA_MALFORMED` because the decoder does not expose a
  distinct error variant.
- The declared uncompressed size is never allocated up front and never trusted as a limit.
- `Error::IoError` carries two different meanings and is not mapped by string matching:
  when the capped writer's sentinel is set the import fails with
  `DNA_DECOMPRESSION_LIMIT`; otherwise (for example `UnexpectedEof` from a truncated stream)
  it fails with `DNA_LZMA_MALFORMED`. `Error::LzmaError` and `HeaderTooShort` also map to
  `DNA_LZMA_MALFORMED`.
- The decoder is applied to one packet payload at a time, not to the file.

**XML path.**

- Decode the packet payload as UTF-8; reject invalid UTF-8 as `DNA_XML_MALFORMED`. No legacy
  encoding is negotiated because `quick-xml`'s `encoding` feature is off and reference notes
  describe these blocks as UTF-8 (fixture 100 confirms it; if a fixture shows another encoding,
  this is raised to `lead` rather than handled by an implicit transcode).
- Parse with `Reader::from_str` (zero-copy slice reader) and these settings:
  `check_end_names = true`, `expand_empty_elements = false`, `allow_dangling_amp = false`,
  text trimming off. Defaults already match; the adapter sets them explicitly so a
  `quick-xml` default change cannot silently weaken parsing.
- Reject `Event::DocType` with `DNA_XML_MALFORMED` ("DTD not allowed"). There is no
  external entity resolver, so no network or filesystem access is possible even if a DTD is
  present. `Decl`, `PI` and `Comment` are ignored: they carry no data this adapter needs
  and no entity semantics.
- Resolve `Event::GeneralRef` only for `amp`, `lt`, `gt`, `apos`, `quot` and for numeric
  character references (`resolve_char_ref()`). Any other name fails with
  `DNA_XML_MALFORMED`; no custom entity is ever looked up, so recursive expansion and
  billion-laughs style input cannot occur.
- Depth: our own counter over `Start`/`End`; above `max_xml_depth` fails with
  `DNA_XML_DEPTH_EXCEEDED`. `quick-xml` has no depth cap of its own. `expand_empty_elements`
  stays off so `Empty` does not create a hidden depth level.
- Element size: accumulate decoded text bytes per element; above `max_xml_element_bytes`
  fails with `LimitExceeded`. CDATA counts toward the same budget.
- Node count: elements plus attributes are counted before a node structure is allocated; above
  `max_xml_nodes` fails with `LimitExceeded` naming the limit. The count is per XML block.
- Import-wide bytes: decoded XML bytes are accumulated across every tokenized XML block of one
  import; above `max_xml_bytes` fails with `LimitExceeded` before tokenizing the block that
  would cross the budget. History blocks are charged for their decoded (decompressed) size.
- Byte offsets: record the start offset as `buffer_position()` before an event and the end
  offset after it; add the packet payload's absolute file offset. On a tokenizer error use
  `error_position()` plus the same base. Reports always carry absolute file offsets.
- No XPath, XSLT, schema validation or DOM is used. The token stream is consumed once.

## 3. Container layout (section 6.1)

### 3.1 Cookie packet

The file starts with a packet header: one type byte and a four-byte big-endian payload
length, then the payload. The first packet must be the cookie.

| Offset | Width | Field | Status | Verified value / rule | Evidence |
| --- | --- | --- | --- | --- | --- |
| 0 | 1 | packet type | V | `0x09` | corpus |
| 1 | 4 | payload length | V | unsigned 32-bit big-endian; 14 in the corpus; counts payload bytes only, excludes the 5-byte header | corpus |
| 5 | 8 | magic | V | ASCII `SnapGene`, no NUL terminator | corpus |
| 13 | 2 | sequence type | V | unsigned 16-bit big-endian; `1` = DNA in the corpus (kind = `dna`); other values remain **H** | corpus |
| 15 | 2 | export version | V | unsigned 16-bit big-endian; `15` in the corpus; recorded in provenance | corpus |
| 17 | 2 | import version | V | unsigned 16-bit big-endian; `19` in the corpus; read but not stored (no canonical field) | corpus |

The corpus payload is `53 6e 61 70 47 65 6e 65 00 01 00 0f 00 13`: the magic followed by
big-endian `(1, 15, 19)`. Little-endian would read `(256, 3840, 4864)`, which is not
plausible; the corpus therefore confirms big-endian for all three fields as well.

Do not encode these offsets in the reader. They are a description for fixture authors and
reviewers. The reader reads the cookie as a declared-length payload, verifies its prefix, and
then reads the three 16-bit fields from the payload only after the payload length has been
confirmed to be large enough. If a future file shows a different payload length, a different
field order or a larger cookie, only the payload decoder changes; the TLV walk is unaffected.

Still open (section 10): a cookie payload longer than 14 bytes, and the numeric sequence types
for RNA/protein. The corpus only proves type 1 = DNA.

### 3.2 TLV stream

After the cookie, packets are a flat sequence of:

```text
type:   1 byte
length: 4 bytes, big-endian (V)
payload: length bytes
```

There is no nesting, no alignment padding required by the reader and no index. The next
packet starts immediately after the previous payload. This is verified by the corpus: the
seven packets tile the file exactly from offset 19 to offset 27646 with no gap and no
trailing bytes (V). Packet order is not sorted by type in the corpus (types 0, 3, 8, 10, 5,
13, 6), so the reader must not assume an order.

### 3.3 Reader algorithm

1. If fewer than 5 bytes are available for the cookie header, or the declared cookie length
   exceeds the remaining bytes, fail with `DNA_TRUNCATED_HEADER` at offset 0 (R).
2. If the cookie type is not `0x09`, or the payload does not start with `SnapGene`, fail
   with `DNA_BAD_MAGIC` at offset 0 (R). The magic check runs before any version field is
   read.
3. If the cookie payload is too short to contain the magic and the three 16-bit fields, fail
   with `DNA_TRUNCATED_HEADER` (R). A payload longer than the fields is accepted; trailing
   cookie bytes are ignored and are not preserved.
4. Read the sequence type, export version and import version. The export version becomes
   `Provenance.export_version`; the sequence type drives the kind check (section 5.6); the
   import version is read for completeness and is not stored, because the canonical model
   has no field for it. Record the cookie packet as packet index 0.
5. Walk the TLV stream from the running offset:
   - If fewer than 5 bytes remain, go to step 7.
   - Read the 4-byte length as `u64` via checked widening. If the length exceeds the
     remaining bytes, fail with `DNA_TRUNCATED_PACKET` at the packet start.
   - If `5 + length` overflows the address space, fail with `DNA_PACKET_LENGTH_INVALID`
     (R; unreachable on 64-bit with a valid file, but the arithmetic is checked).
   - Increment the packet count; above `max_snapgene_packets` fail with
     `LimitExceeded { limit: "max_snapgene_packets" }`.
   - Record `{ offset, packet_type, packet_index, payload_range }` and dispatch by type
     (section 4).
   - Advance the offset by `5 + length`. The reader never seeks and never computes an offset
     from a hardcoded constant.
6. A packet with `length == 0` is valid TLV. Its type-specific decoder decides whether zero
   payload is acceptable; the TLV layer does not reject it.
7. Trailing bytes:
   - If the offset equals the input length, done.
   - If the remaining bytes are all `0x00`, treat them as padding and ignore them (R). This
     covers block-aligned zero fills without letting a zero run parse as packets.
   - If fewer than 5 non-zero bytes remain, emit `DNA_TRAILING_BYTES` (warning) at the offset
     and stop (R). The sequence remains usable.
   - Otherwise the remaining bytes start a packet and step 5 applies; a declared length that
     exceeds the remaining bytes is `DNA_TRUNCATED_PACKET`, not "trailing bytes".

Packet count is bounded by the limit, not by file size alone; a file of many zero-length
packets is rejected by `max_snapgene_packets` rather than looping.

## 4. Packet matrix (section 6.2)

Types 0, 3, 5, 6, 8, 10 and 13 are observed in the calibration corpus (V). Types 1, 7, 16,
17 and 29 remain reference-notes hypotheses (H) until a fixture produces them. The v1
disposition is binding once written here. The corpus packet table with offsets, lengths and
payload digests is in `snapgene-corpus-findings.md`; the corpus contains no history, trace or
alignable packet.

| Type | Content | Status | v1 disposition | Destination | Diagnostics |
| --- | --- | --- | --- | --- | --- |
| 0 | DNA sequence, text bases | V | Decode to `sequence` | record | `DNA_SEQUENCE_INVALID_CHAR` on bad base |
| 1 | DNA sequence, 2-bit packed | H | Decode to `sequence` | record | `DNA_COMPRESSED_LENGTH_INVALID`, `DNA_SEQUENCE_INVALID_CHAR` |
| 3 | Restriction-enzyme recognition-site list | V | Preserve raw; do not parse | report `preserved` or `dropped` | `DNA_PACKET_IGNORED` (info) |
| 5 | Primers XML | V (empty list observed) | Parse primers | record features; `extras.snapgene.primers` | XML codes on failure |
| 6 | Notes XML | V | Preserve as ordered fields | `extras.snapgene.notes`; may fill `name`/`description` | XML codes on failure |
| 7 | History tree, XML or LZMA+XML | H | Decode for validation only; preserve raw | report `preserved` or `dropped` | `DNA_HISTORY_PRESERVED_RAW` (info), `DNA_HISTORY_DECODE_FAILED` (warning) |
| 8 | Sequence properties XML | V | Preserve as ordered fields | `extras.snapgene.properties` | XML codes on failure |
| 10 | Features XML | V | Parse to canonical features | record features | XML codes; `DNA_FEATURE_RANGE_INVALID` per skipped feature |
| 13 | Enzyme settings block ("Unique 6+ Cutters") | V | Preserve raw; do not parse | report `preserved` or `dropped` | `DNA_PACKET_IGNORED` (info) |
| 16 | Trace container, binary | H | Preserve within budget, else fingerprint | report `preserved` or `dropped` | `DNA_PACKET_IGNORED` (info) |
| 17 | Alignable sequences XML | H | Preserve raw; do not interpret | report `preserved` or `dropped` | `DNA_PACKET_IGNORED` (info) |
| 29 | History modifier, LZMA+XML | H | Same as type 7 | report `preserved` or `dropped` | same as type 7 |
| other | Unknown | H | Preserve within budget with type and offset; never guess | report `preserved` or `dropped` | `DNA_UNKNOWN_PACKET` (info) |

Rules:

- Unknown packets do not abort the file. `ImportOptions.preserve_unhandled_blocks` controls
  raw preservation: when true the payload goes to `preserved[]` as base64 when the
  `max_preserved_block_bytes` budget allows; otherwise the block is fingerprinted into
  `dropped[]` with reason `not_requested` (flag disabled) or `size_limit` (budget). Either
  way the record is returned and the paired `DNA_PACKET_IGNORED`/`DNA_UNKNOWN_PACKET`
  diagnostic is emitted.
- Known-but-uninterpreted types 3, 13, 16, 17 and 29 get `DNA_PACKET_IGNORED` (info), including
  when they are preserved. "Ignored" means "not mapped into the model", not "discarded".
- Types 3 and 13 are known metadata blocks that v1 does not model. Their payloads are
  preserved in `preserved[]` within `max_preserved_block_bytes`, with the packet type and
  offset, and reported as `DNA_PACKET_IGNORED` (info). They are not "unknown packets": the
  type is recognized, we simply do not interpret the content.
- A handler that fails on a preserved block is a warning plus fingerprint for optional blocks
  (history, traces, alignable, unknown). A handler that owns required data (types 0, 1, 10,
  5, 6, 8) fails the import with the corresponding structural or XML error, because silently
  losing features or the sequence is worse than rejecting the file.
- Order: packets are processed in file order. Diagnostics are sorted deterministically after
  processing (section 9), so processing order does not leak into report order.
- At most one DNA packet is expected. Types 0 and 1 are alternatives, not layers (section 5.4).
- The corpus shows packet streams are not sorted by type; nothing may depend on order beyond
  "topological order in the file".

### 4.1 History decode path depends on the export version

`export_version < 15`: the history payload is plain XML.

`export_version >= 15`: the history payload is LZMA-compressed XML.

- The path is chosen from the cookie's export version, which is a file-level field. It is not
  chosen by sniffing the payload.
- The corpus has `export_version = 15` and **no history packet at all** (V). The version
  number only selects the decode path when a history packet exists; it is not a promise that
  one exists. A container with version 15 and no type 7/29 packet produces no history
  diagnostic and no history preservation entry.
- No "try LZMA, on failure fall back to XML" is the primary path. Fixtures 010 and 011 must
  each confirm their version's encoding independently.
- The version rule is primary. If the version-selected decode fails, the reader may try the
  other path once as a defensive fallback. A successful fallback emits
  `DNA_HISTORY_DECODE_FALLBACK` (info); a failed one emits `DNA_HISTORY_DECODE_FAILED`
  (warning). In both cases the original payload is preserved or fingerprinted and the record
  is returned. The fallback is never the first attempt.

### 4.2 Export version handling

- A parseable `export_version`, including one newer than the highest version validated so
  far, is accepted as long as the container parses. The value is recorded in provenance and
  there is no separate "unsupported version" code for a newer value (O-13).
- The history split at 15 still applies; a newer version stays on the `>= 15` side unless a
  fixture shows a change. Unknown packet types are preserved and reported with
  `DNA_UNKNOWN_PACKET`; they never fail the import.
- An unparseable or truncated version field fails with `DNA_EXPORT_VERSION_UNSUPPORTED`
  (error) at the cookie offset.
- Corroborated by the corpus for version 15 (V). Older and newer versions, and every sequence
  type other than 1, remain unverified (section 10).
- The `import_version` field (19 in the corpus) has no known semantics and no canonical
  destination. v1 reads and validates its width only; it is not stored and does not gate
  parsing.

## 5. DNA packets, kind and topology (section 6.3)

### 5.1 Packet payload shape

Type 0 is verified from the corpus; type 1 remains hypothesised. Both are believed to begin
with a one-byte flags field because that is what the corpus shows for type 0:

| Field | Status | Verified value / hypothesis | Evidence / confirming fixture |
| --- | --- | --- | --- |
| flags byte (type 0) | V | first payload byte; bit 0 = circular; corpus value `0x1f` | corpus |
| type 0 bases | V | remaining bytes are ASCII uppercase bases, one byte per base; corpus 5369 bytes, ACGT only | corpus |
| flags byte (type 1) | H | assumed present with the same position; width unverified | 002 |
| type 1 length | H | a length field precedes the packed bases; width and endianness unverified | 002 |
| type 1 bases | H | four bases per byte, 2 bits each; bit order unverified | 002 |
| type 1 padding | H | unused low or high bits of the final byte; value and placement unverified | 002 |

### 5.2 Type 0: text bases

- Corpus (V): payload length 5370 = 1 flags byte + 5369 bases; the base region is uppercase
  ASCII `ACGT` only, with no lowercase, ambiguity codes, whitespace or NUL. The decoded
  sequence is 5369 bp starting `ATC` and ending `GCCA`.
- Decode the payload after the flags byte as UTF-8. Invalid UTF-8 fails with
  `DNA_SEQUENCE_INVALID_CHAR` at the packet offset; after normalisation (below), a character
  outside the IUPAC DNA set fails with the same code. The message names the byte position
  inside the packet, never the base content.
- Allowed bases are the canonical DNA set `ACGTNRYSWKMBDHV` (case-insensitive). `N` and the
  ambiguity codes are legal; that is exactly why this encoding exists rather than only 2-bit.
- `U` in a DNA record is invalid: `DNA_SEQUENCE_INVALID_CHAR` (a `.dna` file whose cookie
  says DNA cannot carry RNA bases).
- Case and whitespace are normalised, not rejected: strip ASCII whitespace, uppercase the
  bases, and emit `DNA_SEQUENCE_NORMALIZED` (info) once per packet when anything changed
  (approved code, O-5). Normalisation is a recorded event, not silent rewriting.
- Digits, NUL and any character outside the IUPAC set after normalisation fail with
  `DNA_SEQUENCE_INVALID_CHAR`.
- A packet that decodes to zero bases fails with `DNA_SEQUENCE_EMPTY` (error) at the packet
  offset (approved code, O-4). An empty sequence is not a valid record.

### 5.3 Type 1: 2-bit encoding (hypothesis)

The candidate mapping, taken from reference notes and **not yet confirmed**, is:

| 2-bit value | Base |
| --- | --- |
| 0 | G |
| 1 | A |
| 2 | T |
| 3 | C |

The mapping is unusual precisely because it must not be assumed. A fixture pair is the
confirmation method: build 001 (type 0) and 002 (type 1) from the same known synthetic
sequence, import both, and compare sequences byte for byte. The sequence must be chosen to
discriminate all four values in each of the four positions, for example a sequence containing
`GATC`, `CTAG`, `AGCT` and `TCGA` runs.

Items fixtures must confirm before the decoder may ship:

1. **Mapping**: which 2-bit value is which base. Candidate: `0=G, 1=A, 2=T, 3=C`.
2. **Bit order**: whether the first base occupies the high two bits of a byte
   (MSB-first) or the low two bits (LSB-first).
3. **Length field**: whether a length prefix exists, its width (1/4/8 bytes), endianness, and
   whether it counts bases or packed bytes.
4. **Padding**: which bits of the final byte are padding, and whether they must be zero,
   ignored, or rejected when non-zero.
5. **Self-consistency**: the decoded base count must equal the length the packet declares and
   must be consistent with the largest feature coordinate. A mismatch is
   `DNA_COMPRESSED_LENGTH_INVALID` (error). Two disagreements are errors, not a choice:
   the reader does not truncate to the shorter value.
6. **Uppercase output**: 2-bit encoding cannot represent ambiguity codes, so type 1 output is
   always in `GATC`.

Until items 2-4 are confirmed, the decoder is not written. A wrong bit order produces a
plausible but wrong sequence, which is worse than a typed error, so the implementation must
not "try both orders and keep the plausible one".

### 5.4 Duplicate and missing DNA packets

- Any second DNA packet in one file, whether it is type 1 after type 0, type 0 after type 1,
  or the same type twice, fails with `DNA_DUPLICATE_DNA_PACKET` (error) at the second
  packet's offset. The reader does not pick one, merge them, compare them or keep the first
  (O-3).
- No DNA packet at all: `DNA_NO_DNA_PACKET` (error) at offset 0, because there is no packet
  to point at.
- A DNA packet that decodes to zero bases: `DNA_SEQUENCE_EMPTY` (error) at the packet offset
  (O-4). The canonical record never carries an empty sequence.
- Corpus (V): exactly one type 0 packet and no type 1 packet, so the duplicate branches are
  not exercised. `diagnostics.md` currently words `DNA_DUPLICATE_DNA_PACKET` as "both a type 0
  and a type 1 packet are present", which is narrower than the O-3 decision (any second DNA
  packet). Reported to `lead` as a contract wording gap; the adapter follows O-3.

### 5.5 Flags and topology

- The corpus flags byte is `0x1f` (`0b0001_1111`) on the circular pET-28a(+) plasmid (V).
  Bit 0 = 1 therefore means circular. Bit 0 = 0 meaning linear is the working assumption and
  is pinned by fixture 001; the corpus cannot confirm the zero case.
- Bits 1-4 are also set in the corpus. Their semantics are unknown; a single file cannot
  distinguish a reserved "normal" pattern from a semantic flag. v1 records the full flags
  byte in the report for every DNA packet and treats `0x1f` as the known-normal mask: set
  bits outside that mask (bits 5-7) emit `DNA_TOPOLOGY_FLAG_UNKNOWN` (info) once per DNA
  packet. This mask is a calibration assumption, not a verified fact; a fixture with other
  flags values must confirm or replace it.
- If the flags byte is absent (payload shorter than one byte), the payload is structurally
  invalid for its type: `DNA_TRUNCATED_PACKET` for the packet, unless a fixture shows a
  legitimate flags-less encoding. No real flags-less encoding is known.
- `DNA_TOPOLOGY_MISSING` (error) is reserved for the case where the format genuinely cannot
  supply topology and `ImportOptions.topology_hint` is also absent. No real flags-less
  encoding is known, so fixture 133 has no file construction (O-12): a unit test synthesizes
  the no-topology signal directly, and `conformance` registers that trigger in place of a
  `.dna` file.
- `topology_hint` is consulted only when the parsed value is absent, never to override a
  parsed value (D16). An unknown topology value is `RECORD_FIELD_INVALID`, but the parsed
  bit cannot produce an unknown value.

### 5.6 kind from the cookie

- The cookie's `sequence type` selects `kind`. It is the only kind source for `.dna`, plus
  `ImportOptions.kind_hint` when the field is absent.
- The corpus confirms `sequence type = 1` -> `kind = dna` (V). The numeric values for RNA
  and protein are not verified. The strict v1 rule (lead decision 2026-10-21) is: **any value
  other than 1 fails with `DNA_UNKNOWN_SEQ_TYPE`**. `DNA_UNSUPPORTED_MOLECULE` stays
  reserved until a real non-DNA file verifies the enum values.
- v1 never produces `kind = rna` or `protein`, even though the schema enum accepts them (D15).
- A missing cookie type field is a container truncation error (`DNA_TRUNCATED_HEADER`), not a
  `kind` source question.

### 5.7 name from notes

Priority, highest first:

1. Notes `AccessionNumber`.
2. Notes `CustomMapLabel` when `UseCustomMapLabel` is `1`.
3. Notes `Comments`, as plain text.
4. `ImportOptions.name_hint`, only when the file provides no name. Hints are consulted only
   for missing values; a parsed name always wins (D16).

Corpus evidence (V): the file has no `AccessionNumber`, `CustomMapLabel` = `pET-28a(+)` and
`UseCustomMapLabel` = `1`, so the record name is `pET-28a(+)`. `Comments` is an HTML fragment
that renders empty and would not be a usable name.

If the chosen value is empty or whitespace, continue to the next source. If none produces a
non-empty value: `RECORD_REQUIRED_FIELD_MISSING { field: "name" }` at the notes packet offset,
or at offset 0 when there is no notes packet. The name is not synthesised from the file name,
the target vector, the sequence or a timestamp.

The notes element names in this precedence are verified by the corpus; the fallback branches
(`UseCustomMapLabel = 0`, `AccessionNumber` present) are not exercised and stay part of
fixture 040/130.

`molecule_type` is not represented in `.dna`; it stays `null` (canonical model), and the
GenBank writer derives it later only if asked to write GenBank.

### 5.8 Feature coordinates in DNA packets

- The sequence length is the decoded base count. Feature ranges are converted in one place
  (section 6.3) and validated against that length. Corpus (V): 5369 bases; all feature ranges
  lie inside `1..5369`; the largest end is 5358.
- Linear record, `end > length`: skip the feature, add a `dropped` entry and emit
  `DNA_FEATURE_RANGE_INVALID` (warning). Per D11 this does not abort the file.
- Circular record, `end > length`: keep one unwrapped segment `[start0, end0)` per
  `<Segment>` with `end0 > length`, per D2 as revised on 2026-10-23. A single `<Segment>`
  keeps `operator = span`; a multi-`<Segment>` feature keeps `operator = join` in document
  order. `Segment::resolved` splits the origin on access; the range is not rewritten into two
  origin-split pieces. If the range covers more than one full turn (`end0 - start0 > length`,
  i.e. `B - A + 1 > length`), the location is not expressible: skip and warn.
- `start > end`, negative values, non-numeric ranges or a zero-length range at an unsupported
  position: skip and warn with the same code.
- Cross-origin features are therefore legal input and produce one canonical unwrapped
  segment with `end > length`; they are not an error in circular records.

## 6. XML blocks (section 6.4)

Four packet types carry XML: 5 (primers), 6 (notes), 8 (properties), 10 (features), plus the
history payload of types 7 and 29 after decompression. All of them use the same tokenizer and
the same safety envelope from section 2.8; only the element-to-model mapping differs.

### 6.1 Common shape

- Payload is UTF-8 (V). The corpus roots are `Features` (type 10), `Notes` (type 6),
  `AdditionalSequenceProperties` (type 8) and `Primers` (type 5); none carries a namespace in
  the corpus. The reader matches local names and ignores namespaces, so a future namespace
  prefix does not change mapping; a namespaced block remains unverified (H).
- Unknown elements and unknown attributes inside a known block are ignored but counted toward
  the element-size budget. Feature display attributes and `<V>` custom fields are not lost:
  they are captured in `extras.snapgene.features[]` (section 6.2), not treated as unknown.
- Text values are entity-resolved by the event-level allowlist in section 2.8. No other
  entity is accepted.
- Element order is preserved everywhere it can be: features in document order, segments in
  document order, qualifiers in document order, primers in document order, notes and
  properties as ordered arrays. No sorting is applied to source data.

### 6.2 Features XML (type 10)

Verified shape (V, corpus):

```text
<Features nextValidID="16">
  <Feature recentID="0" name="MCS" type="misc_feature" allowSegmentOverlaps="0" ...>
    <Segment range="158-203" color="#99ccff" type="standard"/>
    <Q name="note"><V text="&lt;html&gt;&lt;body&gt;multiple cloning site&lt;/body&gt;&lt;/html&gt;"/></Q>
  </Feature>
  ...
</Features>
```

- Root `Features` with `nextValidID`; one `Feature` child per feature in document order. The
  corpus has 16 features.
- `Feature` attributes: `recentID` (source ordering hint), `name` (display name), `type`
  (feature key), optional `directionality`, plus type-specific settings that are not mapped
  (for example `translationMW`, `readingFrame`, `allowSegmentOverlaps`,
  `consecutiveTranslationNumbering`, `maxRunOn`, `detectionMode`).
- `Segment` attributes: `range` (required), `color`, `type` (`standard` in the corpus),
  `translated` (present on CDS segments). The corpus has exactly one segment per feature; a
  multi-segment feature remains unverified (H).
- `Q` attributes: `name`. `Q` children: one or more `V` elements carrying either `text` or
  `int`. The corpus has one `V` per `Q` (49/49); multiple `V` children remain unverified.

Mapping:

| SnapGene | Canonical | Rule |
| --- | --- | --- |
| `<Feature>` | one `Feature` | document order preserved |
| `Feature@type` | `feature.key` | kept verbatim; never rewritten |
| `Feature@name` | `feature.name` | absent or empty becomes `null` |
| `Feature@directionality` | `location.strand` | verified: absent -> `1`, `1` -> `1`, `2` -> `-1`; other values are malformed metadata |
| `<Segment range>` | one `Segment` | conversion only via `range_to_segment` (section 6.3) |
| `Segment@color`, `Segment@type`, `Segment@translated`, other attributes | `extras.snapgene.features[]` | display/settings attributes, not canonical |
| `Q@name` | `qualifier.key` | verbatim |
| `V@text` | `qualifier.value` | HTML-normalized plain text: strip tags (block tags become a space), decode entities once, collapse whitespace, trim |
| `V@int` | `qualifier.value` | decimal string of the integer |
| feature without a usable type or segment | skipped feature | `DNA_FEATURE_MALFORMED` + `dropped` reason `malformed_metadata` |

- `directionality`: absent and `1` are forward (`strand = 1`); `2` is reverse
  (`strand = -1`). All three cases occur in the corpus, which verifies the mapping (V).
- Single segment -> `operator = "span"`. Multiple segments -> `operator = "join"` in document
  order (H; not exercised by the corpus). Segment order is never sorted, because join order is
  semantic for reverse-strand features. A single `<Segment range>` that crosses the origin of
  a circular record is one segment with `end > length` (one wrap at most), so it keeps
  `operator = "span"`; the origin split happens on access through `Segment::resolved`, never
  in the canonical payload (D2 as revised on 2026-10-23).
- Qualifier order is document order. Duplicate `Q` names and multiple `V` per `Q` are not
  exercised by the corpus (H for those branches); the canonical qualifier array supports both.
- Text values are HTML documents in SnapGene. The canonical qualifier value is plain text
  produced by the HTML normalization rule (lead decision 2026-10-21, C-8): tags stripped,
  `br` and block-level tags become a space, entities decoded once, whitespace collapsed and
  trimmed. The raw XML-unescaped `V@text` is preserved in
  `extras.snapgene.features[].qualifiers[]`, so nothing is lost. A value with no markup passes
  through verbatim apart from the same whitespace handling.
- `int` values become their decimal string and are never HTML-processed.
- `so_term`: same static key-to-SO table as GenBank; unknown type -> `null`; never guessed
  (O-10).
- A feature whose range cannot be converted, whose coordinates are out of range, or whose
  required metadata is malformed is skipped with a `dropped` entry and a warning; it does not
  abort the file (D11). Range failures use `DNA_FEATURE_RANGE_INVALID`; a missing usable
  key/type or segment uses `DNA_FEATURE_MALFORMED` (warning) with
  `dropped[].reason = "malformed_metadata"` (O-8). A structurally broken features XML
  (unclosed element, depth limit, invalid UTF-8) aborts with `DNA_XML_MALFORMED` /
  `DNA_XML_DEPTH_EXCEEDED`, because the remaining features cannot be trusted.

Feature-level display/settings attributes and per-segment attributes have no canonical home
(O-1/O-2, corrected by calibration: colour is a `Segment` attribute in the corpus, not a
`Feature` attribute). They are stored per feature in `extras.snapgene.features[]`, aligned by
`feature_index`:

```json
{
  "features": [
    {
      "feature_index": 0,
      "key": "misc_feature",
      "attributes": [ { "name": "recentID", "value": "0" } ],
      "segments": [
        {
          "segment_index": 0,
          "attributes": [
            { "name": "color", "value": "#99ccff" },
            { "name": "type", "value": "standard" }
          ]
        }
      ],
      "qualifiers": [
        { "value_type": "text", "raw": "<html><body>the <i>site</i></body></html>" }
      ]
    }
  ]
}
```

`feature_index` is the index into `record.features` and the reference that correlates the
entry with its canonical feature; `key` repeats the source type for stable correlation;
`attributes` are the feature attributes in source order (except `name`, `type` and
`directionality`, which are canonical); `segments[].attributes` are the source attributes of
that segment; `qualifiers[]` is aligned with the feature's canonical qualifiers and records
each source value's `value_type` (`text` or `int`) and its raw XML-unescaped value. The
canonical qualifier stores plain text only, so the raw value is the lossless copy. The array
has an entry only for features that were emitted; skipped features appear in `dropped[]`
instead. `extras.snapgene.dna_flags` always carries the raw DNA flags byte. No schema change
is needed: `extras` is the extension point.

### 6.3 The single range conversion point

All range-to-segment conversion lives in exactly one function:

```text
crates/sequon-io/src/snapgene/xml.rs::range_to_segment
```

No other module parses a range string or adjusts a coordinate. `features.rs` and `primers.rs`
call this function. It returns exactly one segment: a range that crosses the origin of a
circular record stays unwrapped with `end > length` (one wrap at most) instead of being split
at the origin. The caller applies `span` for a single `<Segment>` and `join` for multiple
`<Segment>` children in document order.

The convention is verified (V, corpus): `range="A-B"` is **1-based closed on the recorded
forward strand**. The single conversion is:

```text
start0 = A - 1
end0   = B
```

If `end0 > length`, the record must be circular and `end0 - start0 <= length` (one wrap at
most); the segment is kept unwrapped, because the canonical payload preserves the source
range (section 5.8). A linear record with `end0 > length`, and a circular range covering more
than one full turn, are errors.

The result carries `operator = "span"` and `strand` taken from `directionality`. Evidence:

- MCS `158-203` maps to `seq[157:203]`, 46 bases, which starts with the XhoI site `CTCGAG`
  and ends with the BamHI site `GGATCC`.
- T7 promoter `368-386` maps to `seq[367:386]`, whose reverse complement is the canonical T7
  promoter `TAATACGACTCACTATAGG`.

A one-base shift destroys both checks, so the convention is fixed by evidence, not only by
the absence of a parse error. `range_to_segment` still owns a regression table for the parse
and the circular-wrap cases (unwrapped span, one-turn span bound and the more-than-one-turn
rejection); the convention is no longer a switch between candidates.

Multi-segment ranges (join/order) remain unverified (H) and must be confirmed by fixture 020
before that branch ships.

`range_to_segment` also owns:

- decimal parsing with overflow checks (no `unwrap`);
- rejection of empty, non-numeric, reversed and multi-hyphen ranges;
- keeping a circular origin-crossing range unwrapped per D2 (revised 2026-10-23), with at most
  one wrap;
- the `DNA_FEATURE_RANGE_INVALID` decision for out-of-range results (the caller adds the
  `dropped` entry).

### 6.4 Primers XML (type 5)

Corpus shape (V): the only observed `Primers` document is an empty primer list:

```text
<Primers nextValidID="0">
  <HybridizationParams minContinuousMatchLen="10" allowMismatch="1"
    minMeltingTemperature="40" showAdditionalFivePrimeMatches="1"
    minimumFivePrimeAnnealing="15"/>
</Primers>
```

There are zero `<Primer>` elements. The root name, `nextValidID` and the
`HybridizationParams` settings element are verified; a non-empty primer element, its binding
site, strand and Tm/colour fields remain **H** and need fixture 030. `HybridizationParams` is
settings, not primer data: preserve its attributes in source order under
`extras.snapgene.primer_settings` as `{name, value}` pairs, and never map it to a feature.

- Every primer becomes one canonical feature with `key = "primer_bind"`, `name` from the
  primer name, and `location` from the primer's binding site (strand from the binding site's
  direction). This is the only mapping that lets a primer be visible to consumers without a
  second table.
- The primer's own sequence, Tm, colour, binding sequence, notes and any other attributes are
  not canonical feature fields. They go to `extras.snapgene.primers` as an ordered array
  aligned with the canonical features through `feature_index`.
- Proposed extras shape (field names **H**, confirmed by fixture 030):

```json
{
  "primers": [
    {
      "feature_index": 3,
      "name": "M13F",
      "sequence": "GTAAAACGACGGCCAGT",
      "binding_sequence": "GTAAAACGACGGCCAGT",
      "strand": 1,
      "tm": "58.4",
      "color": "#ff0000",
      "attributes": [ { "name": "note", "value": "..." } ]
    }
  ]
}
```

- Absent values stay absent; the adapter does not invent a Tm, a colour or a strand.
- A primer whose binding site cannot be converted is skipped with a `dropped` entry and
  `DNA_FEATURE_RANGE_INVALID`; its metadata is not emitted, because there is no canonical
  feature to index. The raw primer XML remains available through the preserved packet only if
  the whole packet is preserved; v1 does not separately preserve skipped primer XML.
- Primers are not sorted and are never de-duplicated against features of the same name.

### 6.5 Notes and properties (types 6 and 8)

Notes shape is verified (V): one `Notes` root with 13 child elements in source order:
`UUID`, `Type`, `ConfirmedExperimentally`, `CustomMapLabel`, `UseCustomMapLabel`,
`Description`, `Created` (attribute `UTC`), `LastModified` (attribute `UTC`), `CreatedBy`,
`Organism`, `SequenceClass`, `TransformedInto`, `Comments`. There is no `AccessionNumber`
in the corpus. The description value is an HTML document stored as escaped text, and
`Comments` is an empty HTML fragment.

Properties shape is verified (V): one `AdditionalSequenceProperties` root with four children:
`UpstreamStickiness`, `DownstreamStickiness`, `UpstreamModification`,
`DownstreamModification`. These are ligation/ends settings, not sequence properties.

- Both blocks are preserved under `extras.snapgene.notes` and
  `extras.snapgene.properties` as ordered arrays of `{ name, value, attributes }`, where
  `attributes` is the ordered list of XML attributes on that element (`[]` when none).
  Values are the XML-unescaped source text; the raw HTML markup is preserved here even though
  the canonical fields that may be derived from these blocks store plain text. They are not
  mapped to features and never alter the sequence.
- The only canonical fields notes may fill are `name` and `description` (D16; section 5.7):
  - `name`: `AccessionNumber` -> `CustomMapLabel` when `UseCustomMapLabel` = 1 -> plain
    `Comments` -> `name_hint`. Corpus: `CustomMapLabel = pET-28a(+)` with
    `UseCustomMapLabel = 1` (V).
  - `description`: fill only when `description` is empty and never overwrite a non-empty value
    (O-9). Precedence: notes `Description`, then `Comments` when it was not consumed as
    `name`, then `null`. The canonical value is the plain-text normalization of the field
    (tags stripped, block tags to spaces, entities decoded once, whitespace collapsed and
    trimmed), so it contains no HTML wrapper or inline markup; the raw field remains in
    extras. This is the lead HTML decision of 2026-10-21 (C-8).
- `description` is filled only when empty. A SnapGene import has no GenBank `DEFINITION`, so
  the "do not override DEFINITION" rule cannot trigger here; the rule matters if a future
  path imports both formats.
- Properties never override the cookie, the DNA packet or the flags: the parsed values win
  and the record is not rewritten. A topology disagreement emits `DNA_TOPOLOGY_CONFLICT`
  (warning) at the packet offset (O-14). A length or sequence disagreement has no dedicated
  code; the properties stay verbatim in extras and the parsed values win. Do not add a second
  topology source. The corpus properties contain no topology, length or sequence value, so
  this branch is not exercised (H).
- Unknown note/property elements and attributes are preserved in the same ordered arrays, not
  dropped; they count toward the XML element-size budget.

### 6.6 XML diagnostic codes

| Condition | Code | Severity |
| --- | --- | --- |
| Tokenizer error, invalid UTF-8, DTD, disallowed entity, bad attribute syntax | `DNA_XML_MALFORMED` | error |
| Depth above `max_xml_depth` | `DNA_XML_DEPTH_EXCEEDED` | error |
| Element text above `max_xml_element_bytes` | `LimitExceeded` | error |
| Node count above `max_xml_nodes` | `LimitExceeded` | error |
| Import-wide decoded bytes above `max_xml_bytes` | `LimitExceeded` | error |
| Feature range out of range or unconvertible | `DNA_FEATURE_RANGE_INVALID` | warning |
| Malformed feature metadata (no usable key/type or segment, bad directionality) | `DNA_FEATURE_MALFORMED` | warning + `dropped` reason `malformed_metadata` |
| Unknown element/attribute inside a known block | none (ignored, budget-counted) | - |

These codes apply to the blocks that carry required data or canonical features (types 5, 6,
8 and 10). History, alignable and unknown packets are optional: a tokenizer failure there is
reported as `DNA_HISTORY_DECODE_FAILED` or as the packet's preservation outcome, never as a
fatal `DNA_XML_MALFORMED`. An import is not failed because an optional block that we do not
map is malformed; the raw bytes are preserved or fingerprinted and the record is returned.

The five codes approved on 2026-10-21 (`DNA_SEQUENCE_EMPTY`, `DNA_SEQUENCE_NORMALIZED`,
`DNA_FEATURE_MALFORMED`, `DNA_HISTORY_DECODE_FALLBACK`, `DNA_TOPOLOGY_CONFLICT`) are now in
`diagnostics.md` under `codes_version` `2026-10-21/1`; this document is their adapter-side
specification.

## 7. History, traces and alignable sequences (section 6.5)

### 7.1 History (types 7 and 29)

- Not mapped to a family tree, lineage or any `sequence_relations` structure (D14). The
  construction history is SnapGene application state, not a property of the molecule.
- Corpus (V): the calibration file has no type 7 or 29 packet, even though
  `export_version = 15`. The version threshold is a property of the packet, not a requirement
  that the packet exists; absence produces no history diagnostic.
- Decode path is selected by `export_version` (section 4.1). Version `< 15`: the payload is
  XML. Version `>= 15`: the payload is LZMA-compressed XML. The version comes from the
  cookie, not from sniffing.
- The decoded XML is parsed with the same safe tokenizer and the same depth/size limits, for
  validation only. Its elements are not mapped to features, primers or metadata; it is not
  the current feature list and must not be treated as one.
- Success: emit `DNA_HISTORY_PRESERVED_RAW` (info) and preserve the *original* payload bytes
  (compressed or plain) in `report.preserved[]` when the budget allows. The decoded copy is
  discarded.
- Fallback: if the version-selected decode fails, the other path is tried once (section 4.1).
  Success emits `DNA_HISTORY_DECODE_FALLBACK` (info); failure emits
  `DNA_HISTORY_DECODE_FAILED` (warning). The original bytes are preserved or fingerprinted
  either way, and the record is returned. A broken history never costs the sequence or the
  features.
- Raw preservation is base64 in the import report only, with `what`, `packet_type`, `offset`,
  `length`, `encoding: "base64"`. When `max_preserved_block_bytes` would be exceeded, the
  block is not partially stored: it moves to `dropped[]` with `{what, reason: "size_limit",
  offset, length, digest}` and no payload.
- D7 wins over the older extras example: `extras.snapgene.history` carries metadata only
  (source encoding, byte length, digest, whether the decode succeeded). It never carries a
  base64 blob. The plan section 2.3.6 example that puts base64 under
  `extras.snapgene.history` is superseded by D7 and by the updated canonical model (O-11);
  raw bytes live only in `report.preserved[]` or `report.dropped[]`.
- Type 29 (history modifier) follows the same path. The plan states it is LZMA+XML; whether
  the version threshold applies identically to type 29 is **H** and is covered by fixtures
  010/011 only for type 7. Section 10 registers it.

### 7.2 Traces (type 16)

- Binary, potentially large, and not interpreted in v1.
- Within budget: preserve base64 in `report.preserved[]` and emit `DNA_PACKET_IGNORED`
  (info).
- Over budget: fingerprint into `report.dropped[]` with the SHA-256 of the payload, the
  packet offset, the payload length and `reason: "size_limit"`; emit `DNA_PACKET_IGNORED`
  (info).
- The trace container has its own internal TLV structure per reference notes, but v1 does not
  parse it; the payload is opaque bytes. Parsing it would add a second container format for no
  model benefit.
- A trace packet never fails the import. It is data we choose not to model, not data that
  makes the record invalid.

### 7.3 Alignable sequences (type 17)

- XML in reference notes; v1 does not interpret the alignment.
- Preserve the raw payload within budget, else fingerprint into `dropped[]`, and emit
  `DNA_PACKET_IGNORED` (info).
- No sequence alignment, identity or mapping is derived from this block. Alignments are
  algorithm output, not import-time canonical data.

### 7.4 Preservation budget

One budget, `max_preserved_block_bytes`, covers all preserved blocks in the file (history,
traces, alignable, unknown packets). Blocks are considered in packet order. A block that does
not fit the remaining budget is dropped with a digest; the reader does not evict an earlier
block to make room, and the outcome is therefore deterministic for a given input and limit
set. The rule is written down here because "preserve what fits" without an order would make
the report depend on iteration order.

## 8. Codec use in the reader (section 6.6)

This section is the operator view of the verified configuration in section 2. It repeats the
decision and the assertions so a reviewer does not have to jump between sections.

| Role | Crate | Pinned version | License | Used under | Features |
| --- | --- | --- | --- | --- | --- |
| LZMA decode | `lzma-rs` | `=0.3.0` (2023-01-04) | MIT | `snapgene` only | none |
| XML tokenize | `quick-xml` | `=0.42.0` (2026-08-22) | MIT | `snapgene` only | none (default features off) |

- Both crates are general-purpose codecs, not format parsers. They know nothing about
  SnapGene; all container and element semantics live in this repository. This is the AGENTS.md
  exception, named here with versions and the verification date 2026-10-21.
- `lzma-rs` is used only through `lzma_decompress_with_options` with `CappedWriter`,
  bounded dictionary memory and end-of-stream checking (section 2.8).
- `quick-xml` is used only as a token stream with DTD/entity rejection, depth and element
  caps, and byte offsets (section 2.8).
- No `unsafe` is added by the adapter; `lzma-rs` itself forbids unsafe code.

### 8.1 SnapGene limits

The adapter reuses the plan section 3.6 limits and adds one:

| Limit | Default | Applies to | Source |
| --- | --- | --- | --- |
| `max_snapgene_packets` | 65,536 | TLV packet count | plan 3.6 |
| `max_lzma_output_bytes` | 64 MiB | one decompressed history payload | plan 3.6 |
| `max_lzma_ratio` | 200:1 | decompressed bytes / compressed block length | plan 3.6 |
| `max_lzma_dict_bytes` | **8 MiB** | lzma-rs decoder dictionary (`memlimit`) | **M0 addition to plan 3.6 (O-6)** |
| `max_xml_depth` | 64 | XML element nesting | plan 3.6 |
| `max_xml_element_bytes` | 1 MiB | decoded text of one element | plan 3.6 |
| `max_xml_nodes` | 1,000,000 | elements plus attributes per XML block | shared Limits (M4 hardening) |
| `max_xml_bytes` | 64 MiB | decoded XML bytes across all XML blocks of one import | shared Limits (M4 hardening) |
| `max_preserved_block_bytes` | 8 MiB | total preserved raw bytes per file | plan 3.6 |
| `max_features` | 100,000 | parsed canonical features | plan 3.6 |
| `max_segments_per_feature` | 10,000 | segments per feature | plan 3.6 |

LZMA output and ratio overruns report `DNA_DECOMPRESSION_LIMIT`. A dictionary overrun reports
`DNA_DECOMPRESSION_LIMIT` when the 13-byte LZMA-alone header is present and the pre-check can
run, and `DNA_LZMA_MALFORMED` otherwise (section 2.8). Every other overrun reports the typed
`LimitExceeded` with the limit name and position. Nothing is truncated.

### 8.2 Dependency assertions

These commands are part of the M4 acceptance and the CI dependency gate (plan section 7.8):

```text
cargo tree -p sequon-io --no-default-features     # must not list lzma-rs or quick-xml
cargo tree -p sequon-io                            # default genbank; same assertion
cargo tree -p sequon-io --features snapgene        # must list lzma-rs and quick-xml
```

Additionally the lockfile and manifest are checked to contain none of `gb-io`, `sgffp`,
`biopython` or `@teselagen/bio-parsers`. A failure is a build failure, not a warning.

### 8.3 Re-verification triggers

Re-run the section 2 commands and update this document when:

- a pinned version changes (including a patch);
- `cargo deny check licenses` reports a change in the tree;
- the workspace MSRV changes such that `quick-xml` 0.42.0's rustc 1.86 floor is no longer
  acceptable (downgrade to 0.41.0 and re-verify the API facts);
- `quick-xml` changes its `Config` defaults or its event model in a way this document
  relies on (the pin makes this a deliberate review, not an accident).

## 9. Malicious input and determinism (section 6.7)

### 9.1 Malicious input checklist

| Input | Mechanism | Expected outcome |
| --- | --- | --- |
| Empty or tiny file | length check before any field read | `DNA_TRUNCATED_HEADER` |
| Wrong magic or first packet type | prefix check | `DNA_BAD_MAGIC` |
| Cookie length beyond EOF | declared length vs remaining | `DNA_TRUNCATED_HEADER` |
| Packet length beyond EOF | declared length vs remaining | `DNA_TRUNCATED_PACKET` |
| 4 GiB declared packet length | checked widening, no allocation before the check | `DNA_TRUNCATED_PACKET` |
| Packet count flood (zero-length packets) | `max_snapgene_packets` | `LimitExceeded` |
| Offset arithmetic overflow | checked `u64` arithmetic | `DNA_PACKET_LENGTH_INVALID` |
| LZMA bomb by ratio | `CappedWriter` ratio check | `DNA_DECOMPRESSION_LIMIT` |
| LZMA bomb by absolute size | `CappedWriter` byte limit | `DNA_DECOMPRESSION_LIMIT` |
| LZMA huge dictionary | header pre-check plus `Options.memlimit` from `max_lzma_dict_bytes` | `DNA_DECOMPRESSION_LIMIT` when the header is parsed; otherwise bounded and reported as `DNA_LZMA_MALFORMED` |
| Truncated or corrupt LZMA stream | decoder `Finish` mode; non-sentinel `IoError` | `DNA_LZMA_MALFORMED` (history: warning + raw preserved) |
| Deep XML | own depth counter | `DNA_XML_DEPTH_EXCEEDED` |
| Oversized element text / CDATA | per-element byte counter | `LimitExceeded` |
| DTD or external entity | `DocType` rejection, no resolver | `DNA_XML_MALFORMED` |
| Entity expansion (billion laughs) | only five names + numeric refs accepted | `DNA_XML_MALFORMED` |
| Invalid UTF-8 in XML | UTF-8 validation before tokenizing | `DNA_XML_MALFORMED` |
| Duplicate DNA packet | any second DNA packet, same or different type | `DNA_DUPLICATE_DNA_PACKET` |
| Missing DNA packet | post-walk check | `DNA_NO_DNA_PACKET` |
| DNA packet decodes to zero bases | base-count check | `DNA_SEQUENCE_EMPTY` |
| Lowercase or whitespace in type 0 bases | strip + uppercase, record once | `DNA_SEQUENCE_NORMALIZED` (info) |
| Malformed feature metadata | skip feature, `dropped.reason = malformed_metadata` | `DNA_FEATURE_MALFORMED` (warning) |
| Version-selected history decode fails, other path succeeds | one fallback attempt per block | `DNA_HISTORY_DECODE_FALLBACK` (info) |
| Notes/properties topology disagrees with flags | flags win, record not rewritten | `DNA_TOPOLOGY_CONFLICT` (warning) |
| Feature count / segment count flood | `max_features`, `max_segments_per_feature` | `LimitExceeded` |
| Trailing garbage | trailing rule | `DNA_TRAILING_BYTES` (warning) |
| Non-zero padding bits in 2-bit payload | confirmed by fixture; until then reject or ignore per fixture | `DNA_COMPRESSED_LENGTH_INVALID` or accepted |
| Known-but-unmapped metadata packet (type 3 restriction list, type 13 settings) | preserve raw within budget, record type/offset | `DNA_PACKET_IGNORED` (info) |
| Flags byte with bits outside the observed `0x1f` mask | record raw flags, warn on the excess bits | `DNA_TOPOLOGY_FLAG_UNKNOWN` (info) |
| Unknown packet type | preserve or fingerprint, never parse | `DNA_UNKNOWN_PACKET` (info) |
| Cookie sequence type other than 1 | strict v1 rule (C-9) | `DNA_UNKNOWN_SEQ_TYPE` |
| Non-DNA sequence type (RNA/protein) | reserved; enum values unverified | `DNA_UNSUPPORTED_MOLECULE` (not emitted in v1) |

Every row becomes at least one fixture or unit test by M4. The adapter must return a typed
error for every error row and must not panic, allocate unboundedly or hang. Fuzz target
`fuzz_dna` asserts the same properties over arbitrary bytes.

### 9.2 Determinism

- Two imports of the same bytes with the same crate version produce the same `Record` value
  and the same diagnostic list in the same order.
- Diagnostics are stable-sorted by `(position.offset, code)` after collection
  (plan section 3.7). Two diagnostics with the same offset and code keep the order in which
  their packets were processed, which is file order; this tie-break is deterministic because
  packet processing is sequential.
- Features are emitted in XML document order; segments in range order within a location;
  qualifiers in `Q` document order; primers in primer document order. No sorting is applied
  to source data.
- `extras.snapgene` arrays are ordered arrays, not maps. The namespace object is serialized
  with the canonical profile's map ordering; the payload arrays keep their documented order.
- Base64 for preserved blocks is standard padding, no line breaks, no URL-safe alphabet.
  Digests are lowercase `sha256:` strings.
- The adapter reads no clock, no randomness, no environment variable, no file name, no locale
  and no network. Nothing in the output depends on HashMap iteration order.
- `provenance.source_digest` and `source_bytes` come from the raw input bytes, so a consumer
  can re-parse later; `export_version` comes from the cookie.

## 10. Register of format facts and their status

A fact is listed here until evidence confirms it. Verified rows cite the calibration corpus
(sha256 above); unverified rows keep their fixture requirement. The implementation reads the
unverified rows as a to-do list and must not rely on them.

Calibration on 2026-10-21 verified or partially verified F-1..F-6, F-12..F-19,
F-23 (zero trailing bytes only), F-25..F-29 and F-30 below. The lead decisions changed
adapter policy (codes, limits, extras ownership) and resolved no format fact; the corpus is
the only source of verified format facts. No 2-bit, history, trace or non-empty-primer fact
was promoted.

| ID | Fact | Status | Evidence / remaining confirmation | Impact if wrong |
| --- | --- | --- | --- | --- |
| F-1 | Cookie packet type and magic | verified | corpus: type `0x09`, payload starts `SnapGene` | `detect` and every import fail |
| F-2 | Cookie payload length | verified (14), open (>14) | corpus: 14 bytes; a longer payload is still untested | field reads shift |
| F-3 | Cookie 16-bit endianness | verified | corpus: BE gives `(1, 15, 19)`, LE gives `(256, 3840, 4864)` | sequence type/version garbage |
| F-4 | Sequence type numeric values | partial | corpus: `1` = DNA; RNA/protein/unknown values need 131/132 | kind misclassification |
| F-5 | Flags byte position and width | verified | corpus: one flags byte at payload start | topology and base offset wrong |
| F-6 | Flags bit 0 topology | partial | corpus: bit 0 = 1 on a circular plasmid; bit 0 = 0 -> linear is the fixture-001 assumption | topology inverted |
| F-7 | Bits 1-4 of flags | observed, semantics unknown | corpus: `0x1f`; mask policy is a calibration assumption, fixtures 001/002 must vary flags | unknown bits reported or missed |
| F-8 | 2-bit base mapping | unverified | fixture 002 vs 001 | wrong sequence |
| F-9 | 2-bit bit order | unverified | fixture 002 | wrong sequence |
| F-10 | 2-bit length field | unverified | fixture 002 | length mismatch or wrong decode |
| F-11 | 2-bit final-byte padding | unverified | fixture 002 (1,2,3-base sequences) | trailing base corruption |
| F-12 | Type 0 base encoding | verified for ACGT | corpus: 5369 uppercase ACGT bytes; IUPAC/N and lowercase are fixture 100/001 | invalid base handling |
| F-13 | XML element/attribute names | verified | corpus: `Features/Feature/Segment/Q/V`, `Notes`, `AdditionalSequenceProperties`, `Primers` | mapping needs rename only |
| F-14 | Feature directionality values | verified | corpus: absent and `1` forward, `2` reverse | strand inverted |
| F-15 | Range coordinate convention | verified | corpus: 1-based closed -> `start0 = A-1`, `end0 = B`, checked against XhoI/BamHI and the T7 promoter | one-base shift on every feature |
| F-16 | Range separator and partial markers | verified separator, no partials observed | corpus: `A-B`; no `<`/`>` equivalent exists in the corpus | parse failure |
| F-17 | Primer binding-site fields and strand | unverified | corpus has zero primers; fixture 030 | primers misplaced |
| F-18 | Notes field names | verified for observed fields | corpus: 13 fields, no `AccessionNumber`; fallback branches need 040/130 | name/description source wrong |
| F-19 | Properties field shape | verified | corpus: four scalar children | extras shape only |
| F-20 | History payload framing for version >= 15 | unverified | no history packet in corpus; fixture 011 | decode path wrong |
| F-21 | Type 29 version threshold | unverified | no history packet; additional fixture or corpus-local | history modifier not decoded |
| F-22 | Trace container internals | unverified | corpus has no type 16; fixture 050 | none in v1 (opaque) |
| F-23 | Trailing zero padding convention | partial | corpus: zero trailing bytes after the last packet; padding rule needs fixture | trailing warning noise |
| F-24 | Cookie payload may exceed 14 bytes | unverified | corpus-local or a synthetic variant | cookie field reads guarded by length |
| F-25 | Packet order in the stream | verified | corpus: types 0,3,8,10,5,13,6 (not sorted) | ordering assumptions break |
| F-26 | Type 3 layout | verified framing | corpus: version byte 1, BE u32 list length 4763, comma list of 473 IUPAC sites, opaque tail 6670 bytes | preservation wrong or misparse |
| F-27 | Type 13 layout | partial | corpus: 345-byte opaque block, label "Unique 6+ Cutters" at offset 18; field semantics unknown | none in v1 (opaque) |
| F-28 | Q/V value shape | verified 1:1 text/int | corpus: 49 Q, one V each, 33 text + 16 int, XML-escaped HTML values; duplicate names and multiple V unverified | qualifier values wrong or lost |
| F-29 | Export version vs history presence | verified | corpus: `export_version = 15` with no history packet | wrong history diagnostic or search |
| F-30 | Type 5 root and settings | verified empty list | corpus: `Primers nextValidID` + `HybridizationParams`, zero `Primer` elements | primer parsing shape |

Rows F-8 to F-11 are the highest-risk group: a mistake there yields a plausible but wrong
sequence rather than an error. They must be confirmed by the paired fixture method, not by
"it imports".

## 11. Fixture inventory (plan section 7.3)

Conformance owns `testdata/README.md` and the fixture files; this table is the adapter's
expectation contract. Plan section 7.3 requires 19 fixtures and all 19 remain required; the
corpus did not replace any of them. Every fixture is synthetic, uses a self-authored sequence
and is built in-repo; no byte of the corpus is copied. None is created in M0.

| Fixture | Coverage | Expected outcome | Diagnostic code | Pins |
| --- | --- | --- | --- | --- |
| `001-linear-uncompressed.dna` | type 0, linear, minimal XML, flags `0x00` | record returned, sequence matches source, topology linear | none | F-5, F-6 (zero case), F-12 |
| `002-circular-compressed.dna` | type 1, circular | record returned, sequence equals the type-0 twin | none | F-8, F-9, F-10, F-11 |
| `010-history-v14.dna` | export_version < 15, plain XML history | record returned, history preserved | `DNA_HISTORY_PRESERVED_RAW` (info) | F-20, F-21 |
| `011-history-v17.dna` | export_version >= 15, LZMA history | record returned, history preserved | `DNA_HISTORY_PRESERVED_RAW` (info) | F-20, F-21 |
| `020-features-multisegment.dna` | multi-segment feature, join order | features with `join`, segment order preserved | none | F-13 (multi-segment branch) |
| `030-primers.dna` | type 5 with non-empty primers | `primer_bind` features plus `extras.snapgene.primers` | none | F-17, F-30 |
| `040-notes-properties.dna` | types 6 and 8; name fallback branches; HTML values | extras populated, name/description filled | none | F-18, F-19, F-28 |
| `050-traces.dna` | type 16 within budget | trace preserved base64, record returned | `DNA_PACKET_IGNORED` (info) | F-22 |
| `060-unknown-packet.dna` | unknown packet type | record returned, packet preserved | `DNA_UNKNOWN_PACKET` (info) | packet walk |
| `070-truncated.dna` | packet length beyond EOF | import fails | `DNA_TRUNCATED_PACKET` | F-1, F-2 |
| `080-lzma-bomb.dna` | high-ratio LZMA history | import fails without OOM | `DNA_DECOMPRESSION_LIMIT` | section 2.8 |
| `090-deep-xml.dna` | XML nested beyond `max_xml_depth` | import fails | `DNA_XML_DEPTH_EXCEEDED` | XML safety |
| `100-unicode-notes.dna` | UTF-8 notes plus lowercase/IUPAC type 0 | record returned, text/sequence normalized | `DNA_SEQUENCE_NORMALIZED` (info) when lowercase | F-12, F-18 |
| `110-both-dna-packets.dna` | type 0 and type 1 in one file | import fails | `DNA_DUPLICATE_DNA_PACKET` | section 5.4 |
| `120-no-dna-packet.dna` | metadata packets only | import fails | `DNA_NO_DNA_PACKET` | packet walk |
| `130-missing-name.dna` | notes without name, no hint | import fails | `RECORD_REQUIRED_FIELD_MISSING { field: "name" }` | F-18, D16 |
| `131-rna-seq-type.dna` | cookie sequence type is not 1 (project placeholder 2) | import fails | `DNA_UNKNOWN_SEQ_TYPE` (strict v1 rule) | F-4 |
| `132-unknown-seq-type.dna` | sequence type outside the enum | import fails | `DNA_UNKNOWN_SEQ_TYPE` | F-4 |
| `133-missing-topology.dna` | flags cannot supply topology, no hint | unit-test trigger; no `.dna` construction is known, so conformance registers the synthesized no-topology signal instead of a file | `DNA_TOPOLOGY_MISSING` | F-5, F-6, section 5.5 |

Fixture 133 is the only row without a file construction (O-12); it is realized as a unit test
that injects a flags value with no topology signal. The other 18 rows are files.

### 11.1 Calibration-derived fixtures (additive)

The corpus is never committed, so the facts it verified need synthetic fixtures with
self-authored sequences. These are proposed in addition to the 19 plan fixtures; they do not
replace any row above.

| Fixture | Covers | Expected outcome | Diagnostic code |
| --- | --- | --- | --- |
| `003-type0-observed-flags.dna` | type 0 with flags `0x1f`, circular, notes `CustomMapLabel`, empty features | record with name from `CustomMapLabel`, circular | none (bits 1-4 inside the mask) |
| `004-linear-zero-flags.dna` | type 0 with flags `0x00` | record, topology linear | none |
| `005-unknown-flag-bits.dna` | type 0 with flags `0x21` (bit 5 outside the mask) | record, topology circular | `DNA_TOPOLOGY_FLAG_UNKNOWN` (info) |
| `012-version15-no-history.dna` | export_version 15, no history packet | record, no history entry | none |
| `013-unsorted-packets.dna` | packets emitted out of type order | record, packet order preserved in report | none |
| `021-features-attributes.dna` | feature/segment attributes, Q text/int, HTML-escaped values, directionality absent/1/2 | exact strand, qualifier values, extras attributes | none |
| `031-primers-empty-settings.dna` | `Primers` root with `HybridizationParams`, zero primers | record, `extras.snapgene.primers` empty, settings preserved | none |
| `041-name-custom-map-label.dna` | `CustomMapLabel` with `UseCustomMapLabel=1` | name from CustomMapLabel | none |
| `042-name-precedence.dna` | AccessionNumber + CustomMapLabel + Comments variants | AccessionNumber wins; UseCustomMapLabel=0 falls back | none |
| `061-restriction-sites.dna` | type 3 version 1 + BE list length + IUPAC list + opaque tail | record, payload preserved | `DNA_PACKET_IGNORED` (info) |
| `062-enzyme-settings.dna` | type 13 label + opaque block | record, payload preserved | `DNA_PACKET_IGNORED` (info) |

### 11.2 Remaining coverage needed

Still required and not covered by the corpus or the additions above: 2-bit compressed DNA
(002), non-empty primer element shape (030), multi-segment features (020), history version 14
and 17 including LZMA framing (010/011), traces (050), unknown packet types (060), truncation
and decompression/depth limits (070/080/090), both/no DNA packets (110/120), non-DNA and
unknown sequence types (131/132), missing name and topology (130/133). The extra
diagnostic-code fixtures from the M0 list remain needed: empty DNA packet, lowercase/whitespace
sequence, malformed feature metadata, history fallback, topology conflict, duplicate same-type
DNA packets, non-zero 2-bit padding, oversized element text and zero-length packet flood.

`testdata/paired/` holds a `.dna` and a GenBank export of the same plasmid, produced locally
in SnapGene, for the section 7.5 comparison. It stays local (Q8). `testdata/corpus-local/` is
not committed and is enabled only through `SEQUON_CORPUS_DIR`.

## 12. Lead decisions (2026-10-21)

All items raised in the M0 draft were decided by `lead` on 2026-10-21. They are recorded here
as applied design, not as open questions. The five new diagnostic codes are approved and are
being added to `diagnostics.md` by `contract`; this document is the adapter-side specification
for them.

| ID | Decision | Applied in |
| --- | --- | --- |
| O-1 | Feature display/settings attributes live in `extras.snapgene.features[]` aligned by `feature_index`; calibration corrected "colour" to a per-segment attribute. No canonical field and no schema change. | 6.2 |
| O-2 | Calibration corrected the premise: `<V>` is the qualifier value container (`text`/`int`), not a custom field. Values map to canonical qualifiers and the variant is recorded in `qualifier_value_types[]`. | 6.2 |
| O-3 | Any second DNA packet, same or different type, fails with `DNA_DUPLICATE_DNA_PACKET`; no implicit choice. | 5.4 |
| O-4 | New `DNA_SEQUENCE_EMPTY` (error, offset) when a DNA packet decodes to zero bases. | 5.2, 5.4 |
| O-5 | New `DNA_SEQUENCE_NORMALIZED` (info, offset) when an uncompressed DNA packet needed lowercase/whitespace normalisation; normalise and record, never reject silently. | 5.2 |
| O-6 | `max_lzma_dict_bytes` approved as an additive limit, default 8 MiB, enforced through the lzma-rs `memlimit`; recorded as an M0 addition to plan section 3.6. | 2.8, 8.1 |
| O-7 | New `DNA_HISTORY_DECODE_FALLBACK` (info, offset) when the version-selected decode fails and the other path succeeds; the version rule stays primary. | 4.1, 7.1 |
| O-8 | New `DNA_FEATURE_MALFORMED` (warning, offset + packet) when a feature lacks a usable key/type or segment; skip it with `dropped.reason = "malformed_metadata"`. | 6.2, 6.6 |
| O-9 | `description` is filled from notes only when it is empty and never overwrites a non-empty value; precedence `Description`, then unused `Comments`, then null. Calibration: the notes value is HTML and is stored verbatim, without tag stripping. | 6.5 |
| O-10 | SnapGene feature types use the same static key-to-SO table as GenBank; unknown types get `null`, never a guess. | 6.2 |
| O-11 | D7 wins: `extras.snapgene` holds decoded/structured metadata only; raw preserved bytes live only in `report.preserved[]`/`dropped[]`. The plan section 2.3.6 base64 example under `extras.snapgene.history` is superseded; no base64 blob in extras. | 6.2, 6.5, 7.1 |
| O-12 | Fixture 133 has no known file construction; a unit test synthesizes flags with no topology bit and `conformance` registers that trigger. | 5.5, 11 |
| O-13 | A newer `export_version` is accepted when the container parses; unknown packet types are preserved and reported as `DNA_UNKNOWN_PACKET`; no new version code. An unparseable version keeps `DNA_EXPORT_VERSION_UNSUPPORTED`. | 4.2 |
| O-14 | New `DNA_TOPOLOGY_CONFLICT` (warning, offset) when notes/properties disagree with the DNA flags; flags win and the record is not rewritten. | 6.5 |

No open items remain for the M0 draft. A new question found during M4 is raised as a new row
and does not reopen these decisions.

### 12.1 M4 calibration corrections (2026-10-21)

The corpus corrected several M0 assumptions. All corrections are applied in the sections named
below; none changes an accepted contract, and none promotes an unverified format fact.

| # | M0 assumption | Calibrated fact | Applied in |
| --- | --- | --- | --- |
| C-1 | The `Q` element text is the qualifier value | The value is the child `V@text` or `V@int`; text is XML-escaped and often HTML | 5.7, 6.2 |
| C-2 | Feature colour is a feature attribute | Colour is a `Segment` attribute; feature-level settings are separate | 6.2 |
| C-3 | Range convention undecided | 1-based closed: `start0 = A - 1`, `end0 = B` | 6.3 |
| C-4 | Sequence type values unknown | `1` = DNA | 5.6 |
| C-5 | `export_version >= 15` implies a history path | Version 15 with no history packet; the version selects a path only when a packet exists | 4.1, 7.1 |
| C-6 | Packet matrix from reference notes | Types 3 and 13 are real, known-but-unmapped metadata blocks | 4 |
| C-7 | Unknown flag bits always warn | Observed `0x1f`; v1 treats it as the known-normal mask and warns only outside it | 5.5 |
| C-8 | HTML values kept verbatim in canonical fields | Canonical qualifier and description values are HTML-normalized plain text; the raw `V`/notes values stay in extras | 6.2, 6.5 |
| C-9 | Sequence types split into known non-DNA and unknown | Strict v1 rule: any cookie sequence type other than 1 is `DNA_UNKNOWN_SEQ_TYPE`; `DNA_UNSUPPORTED_MOLECULE` stays reserved | 5.6 |
| C-10 | XML allocation bounded only by depth and element size | Added `max_xml_nodes` (per block, elements plus attributes, checked before node allocation) and `max_xml_bytes` (import-wide decoded bytes, including decompressed history) | 6.1, 8.1 |

Two follow-ups are recorded rather than silently decided:

- **Contract wording gap**: `diagnostics.md` describes `DNA_DUPLICATE_DNA_PACKET` as type 0 +
  type 1, while the O-3 decision covers any second DNA packet. Reported to `lead`; the
  adapter follows O-3.
- **HTML values (resolved)**: the lead decided on 2026-10-21 that canonical qualifier and
  description values are HTML-normalized plain text, with the raw values kept in extras.
  Cross-format comparison in plan section 7.5 therefore compares plain text; no separate
  conformance-side HTML normalization is required for these fields.

## 13. Verification evidence (2026-10-21)

Commands and results used in sections 2, 5-7 and 10. Host paths are omitted; the probe projects
were temporary and outside this repository.

```text
$ cargo info lzma-rs --registry crates-io
version: 0.3.0
license: MIT
rust-version: unknown
features:
  enable_logging = [env_logger, log]
  env_logger     = [dep:env_logger]
  log            = [dep:log]
  raw_decoder    = []
  stream         = []

$ cargo info quick-xml --registry crates-io
version: 0.42.0
license: MIT
rust-version: 1.86
features:
 +default           = []
  arbitrary         = [dep:arbitrary]
  async-tokio       = [tokio]
  encoding          = [encoding_rs]
  escape-html       = []
  serde             = [dep:serde]
  serialize         = [serde]
  tokio             = [dep:tokio]
```

Sparse index release timestamps:

```text
lzma-rs 0.3.0  pubtime 2023-01-04T00:34:47Z
quick-xml 0.42.0 pubtime 2026-08-22T19:22:01Z  rust_version 1.86
```

Manifests in the local cargo registry cache (edition, license, MSRV):

```text
lzma-rs 0.3.0     edition 2018  license MIT                 no rust-version  no build.rs
quick-xml 0.42.0  edition 2024  license MIT                 rust-version 1.86  no build.rs
byteorder 1.5.0   edition 2021  license Unlicense OR MIT    rust-version 1.60
crc 3.4.0         edition 2021  license MIT OR Apache-2.0   rust-version 1.83
crc-catalog 2.5.0 edition 2018  license MIT OR Apache-2.0   no rust-version
memchr 2.8.3      edition 2021  license Unlicense OR MIT    rust-version 1.61
```

Resolved tree for the two pinned dependencies:

```text
lzma-rs v0.3.0
|-- byteorder v1.5.0
\-- crc v3.4.0
    \-- crc-catalog v2.5.0
quick-xml v0.42.0
\-- memchr v2.8.3
```

Security probe (compiled against the pinned versions):

```text
-- dtd --
REJECT: DTD at end-offset 54
-- offsets-and-entities --
start Feature at end-offset 24
empty Segment at end-offset 47
entity-ref "amp" end-offset 69
entity-ref "lt" end-offset 75
entity-ref "gt" end-offset 80
-- malformed --
malformed xml: ill-formed document: expected `</a>`, but `</b>` was found error_position=9
input 1048576 compressed 26034 ratio 40.3
output-cap result: io error: output limit (io=true)
ratio-cap result: io error: ratio limit ratio_hit=true
full decode ok: 1048576 bytes
truncated result: io error: failed to fill whole buffer (lzma_error=false)
```

This last block confirms three things the design relies on: the DTD event is refusable, entity
references arrive as separate events with usable offsets, and a writer that returns an
`io::Error` aborts decompression before the output limit is exceeded.

### 13.1 Corpus calibration evidence (2026-10-21)

Read-only analysis of `testdata/corpus-local/pET-28a(+).dna` (27646 bytes, sha256
`f298f4c87de87d193597771d63f515cb7906f8d71efb0ebc3d920e1a188f6a0d`), standard library only,
script outside the repository and not committed. The full tables are in
`.agents/task/plasmid-sequence-design/snapgene-corpus-findings.md`.

```text
file bytes 27646  sha256 f298f4c8...8f6a0d
cookie type 0x09  length 14  magic "SnapGene"  u16be (1, 15, 19)
packets (idx offset type length payload_offset)
  0 19     0  5370  24
  1 5394   3  11438 5399
  2 16837  8  289   16842
  3 17131  10 9163  17136
  4 26299  5  217   26304
  5 26521  13 345   26526
  6 26871  6  770   26876
end 27646 = file length, trailing 0
type 0: flags 0x1f, bases 5369, alphabet ACGT, sequence sha256 00f6f8d5...901d
type 3: version 1, BE list length 4763, 473 IUPAC comma items, opaque tail 6670
type 13: 345 bytes, label "Unique 6+ Cutters" at offset 18, remainder opaque
features: 16, one Segment each; directionality absent 4 / "1" 4 / "2" 8
range check: MCS 158-203 -> seq[157:203] starts CTCGAG ends GGATCC
             T7 promoter 368-386 -> revcomp == TAATACGACTCACTATAGG
notes: 13 fields; no AccessionNumber; CustomMapLabel "pET-28a(+)"; UseCustomMapLabel 1
properties: UpstreamStickiness, DownstreamStickiness, UpstreamModification, DownstreamModification
primers: Primers nextValidID="0" with HybridizationParams, zero Primer elements
```

No history, trace or alignable packet exists in this file despite `export_version = 15`.
The corpus file is not a fixture: it is not committed and is not copied into `testdata/`.
This document records derived facts and short coordinate/site checks, not the file.
## 14. Implementation map (M4)

The reader is behind the `snapgene` feature of `sequon-io` and is import-only. Modules:

| Module | Responsibility |
| --- | --- |
| `snapgene/mod.rs` | entry point, record assembly, name/topology rules, extras, report |
| `snapgene/container.rs` | cookie and TLV walk, packet positions, trailing-byte rule |
| `snapgene/dna.rs` | type 0 (verified) and type 1 (hypothesis) DNA decoding |
| `snapgene/xml.rs` | safe XML envelope, `range_to_segment`, absolute error offsets |
| `snapgene/html.rs` | HTML-to-plain-text normalization (C-8) |
| `snapgene/features.rs` | features XML to canonical features and extras |
| `snapgene/primers.rs` | primers XML to `primer_bind` features (hypothesis shape) |
| `snapgene/notes.rs` | notes fields, name/description sources |
| `snapgene/properties.rs` | properties fields and topology-conflict check |
| `snapgene/history.rs` | version rule, decode validation, raw preservation |
| `snapgene/codec.rs` | LZMA decode with `CappedWriter` and dictionary/output caps |
| `snapgene/state.rs` | diagnostics, dropped/preserved collections, base64 |

`cargo tree -p sequon-io --no-default-features` contains neither `lzma-rs` nor `quick-xml`;
both enter only with `--features snapgene`. The reader has no writer and no public type beyond
the shared `import` entry points. The local corpus acceptance test runs only when the
gitignored file (or `SEQUON_CORPUS_DIR`) is present.
