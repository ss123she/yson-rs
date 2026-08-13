# Changelog

All notable changes to this crate are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crate follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] — unreleased

Everything since `ba2044c` (published as 0.1.3). **This release is breaking**:
the value tree gained a lifetime, and the constructors take a `YsonFormat`
rather than a `bool`. See [Migrating from 0.1](#migrating-from-01) at the end.

### Fixed

Ten defects, of which six caused silent data loss or invalid output rather
than an error. Each has a named regression test.

- **A container's terminator was the visitor's job to consume, and fixed-length
  visitors do not consume it.** A `Vec` asks for one element past the end, and
  the `None` answering that last question is what read the `]`; a tuple, a tuple
  struct or an array asks exactly its length of times and stops. At the top
  level the bracket was left as trailing data; *nested*, it was read as the
  enclosing container's terminator. `[[1;2];[3;4]]` into `Vec<(i32, i32)>`
  returned `[(1, 2)]` — half the rows, no error. `{k=[1;2];j=9}` into a struct
  failed outright. The deserializer that writes an opening token now consumes
  the closing one, and a container longer than the visitor read — `[1;2;3]` into
  `(i32, i32)` — is an error instead of a silent truncation.
  (`tests/container_tests.rs`)

- **Decoding an attributed map into `YsonValue` dropped the body.** An
  attributed value reaches the visitor flattened, and the visitor treated one
  `@`-key as a mode switch: every plain key after it was collected and
  discarded. `<a=b>{x=10}` — the shape of every attributed cluster response —
  decoded to an attributed *entity*. The reading is now a total function over
  the three independent facts in that stream (attributes present, `$value`
  present, plain keys present); `$value` beside plain keys is an error naming
  the offending key. (`tests/attributed_value_tests.rs`)

- **`from_slice` read the front of a document and called it the whole.**
  `42 garbage` answered `Ok(42)`, so a truncated or concatenated document was
  indistinguishable from a healthy one. The whole slice must now be the value,
  insignificant whitespace and comments aside, and the error names the offset of
  the first trailing byte. (`tests/exhaustion_tests.rs`)

- **Three struct shapes serialized to output this crate's own parser rejects.**
  An empty struct emitted **zero bytes**; an all-attribute struct emitted
  `<x=1>` with no value node; an `@`-renamed field declared after a plain one
  emitted `{a=1<x=2>}`; and a `$value` field beside plain fields emitted
  `{a=12}`, two bodies run together into one number. Struct serialization is now
  a state machine over the one shape YSON allows — optional attributes, then
  exactly one body — so these produce `{}`, `<x=1>#`, and errors that name the
  offending field. (`tests/struct_shape_tests.rs`)

- **A varint longer than `u64` decoded to a wrong number.** The tenth byte of a
  `u64` varint carries only the top bit; a payload past it was shifted out
  silently, so malformed input produced a plausible wrong value. Ten-byte
  varints that do fit — `u64::MAX` is one — still decode.
  (`src/core/varint.rs`)

- **Running out of bytes mid-varint was reported as malformed, not as short.**
  `YsonError::UnexpectedEof` exists for exactly this, and the varint reader
  returned `Custom` instead, so a binary stream truncated inside a length prefix
  looked corrupt rather than incomplete. Framing could not tell "read more" from
  "give up". The offset is now absolute rather than relative to the varint.

- **A truncated `%` special value was reported as malformed.** `%tr` is a prefix
  of `%true`, not a syntax error, and calling it one made framing a text stream
  fail whenever a boolean straddled a buffer boundary. A prefix of `true`,
  `false`, `nan`, `inf` or `-inf` is now a short read; anything else is still an
  error. (`src/core/frames.rs`)

- **`\a`, `\b`, `\f` and `\v` decoded to the letter, not the control
  character.** The specification calls text strings C-escaped; those four were
  missing from the escape table and fell through to the catch-all, so `"\b"`
  decoded to `b` rather than to `0x08` -- silently, with no error. `\'` and
  `\?` are handled too, completing the C set.
  (`tests/spec_conformance_tests.rs`)

- **A bare decimal above `i64::MAX` was rejected.** The spec lists an unsuffixed
  number as a text form of *both* int64 and uint64, so one that does not fit the
  first can only be the second. `18446744073709551615` now reads as a `uint64`
  instead of erroring; this crate's own writer always emits the `u` suffix, so
  only input from other producers was affected.

- **`Writer` could emit text output that was not valid UTF-8.** A string node
  holding non-UTF-8 bytes was written through raw, which is legal YSON but made
  `to_string` refuse the crate's own output, and made the same value spell
  differently through `Writer` than through serde. Non-UTF-8 bytes are now
  hex-escaped: text output is always valid UTF-8, and the two paths agree
  byte for byte. Found by the Go interop fixtures.

Two further defects reported against 0.1.3 were already fixed in the working
tree before this changelog began, and now have regression tests: a stray `/` in
text input looped forever in `skip_ignored`, and non-UTF-8 map keys and
attribute names were rejected or silently renamed to `""`.
(`src/core/reader.rs`, `tests/regression_tests.rs`)

### Added

- **The value tree borrows its input.** `YsonValue<'a>` and `YsonNode<'a>` hold
  `Cow<'a, [u8]>` for every byte string — string values, map keys, attribute
  names — so reading a document copies none of its payload. Reading a one
  megabyte string costs the tree node that points at it and nothing else; on the
  benchmark corpus the untyped read path is **2.0× faster** than the copying one
  it replaces. `YsonValue::into_owned` detaches a tree from its buffer, and
  `OwnedYsonValue` is the `'static` alias for the result.

  Two cases genuinely cannot borrow and still allocate: a text string carrying
  backslash escapes, whose decoded bytes exist nowhere in the input, and a value
  built by hand. Both are measured rather than asserted —
  `tests/zero_copy_tests.rs` uses a counting global allocator to pin the exact
  numbers, including that a 1 MB string and a 4-byte string cost identically.

- **`core::scan`** — `scan_value` reports the byte length of the first complete
  value in a buffer, or `Scan::Incomplete` if more bytes are needed. It walks
  the token stream without building values and **never allocates**, which makes
  it about 3.6× faster than reading. This is the primitive that makes an input
  larger than memory possible; a truncated value is `Incomplete` rather than an
  error, because on a stream "the buffer ends here" is normal.

- **`core::frames`** — `Frames` cuts a list fragment held in memory into one
  borrowed slice per value and is an ordinary `Iterator`; `FrameReader<R: Read>`
  does the same off a pipe, refilling and compacting a buffer as it goes. This
  is the loop every consumer of a YTsaurus job stream would otherwise write for
  itself. It bounds a single record (`with_max_record_bytes`, so a corrupt
  length prefix cannot buffer until the process dies), retries interrupted
  reads, reports a stream that stops mid-record, and carries lengths rather than
  borrows between calls so the buffer is never frozen while a caller decides
  what to do.

  Neither type knows anything about the YTsaurus protocol. Control records, key
  switches and row indices belong in a job harness.

- **`Serialize` for `YsonValue` and `YsonNode`.** The tree could only be decoded
  into, never encoded from, so a decode → encode round trip was impossible
  through serde and a `YsonValue` nested in a struct could not be written at
  all — which is what a pass-through job needs. Valid UTF-8 takes the
  `serialize_str` path so text output can use unquoted identifiers; everything
  else takes `serialize_bytes`. A test asserts the serde path and
  `Writer::write_value` emit identical bytes in both formats.

- **`Reader::skip_token`** and `TokenKind` — advancing past a token while
  reporting only its shape, so scanning never decodes an escape it has no use
  for. A type that cannot carry a value cannot allocate one.

- **`Serializer::with_buffer`** — appending several values into one allocation,
  for a caller writing a stream.

- **Non-panicking DOM accessors**: `as_u64`, `as_f64`, `as_bool`, `as_map`,
  `get`, `get_bytes`, and `YsonValue::string` / `YsonNode::string`
  constructors, beside the panicking `Index`.

- **`#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]` on `YsonFormat`.** It
  is a fieldless enum that every entry point takes by value; without `Copy` it
  could not be reused across two calls.

### Changed

- **The crate is two layers.** `core` is the format — reader, writer, scanner,
  framer, value tree — with no serde and no `str`. `surface` is the serde
  binding, behind the default-on `serde` feature. `core` builds with
  `default-features = false`.

- **Constructors take a `YsonFormat`, not a `bool`.** `Deserializer::new(input,
  format)` replaces `from_bytes(input, is_binary)`, and `Serializer::new(format)`
  replaces `new(is_binary)`. `Deserializer::with_format`, `Serializer::with_format`
  and `YsonFormat::from_is_binary` are gone: there was one concept with two
  spellings, and the `bool` spelling was the one that reads wrong at call sites.

- **`deserialize_struct` flattens a struct whose only special field is
  `$value`.** Keying only on `@` meant a struct with a single `$value` field
  serialized to `7` and then failed to read `7` back — the crate could not parse
  what it had just written.

- Map keys and attribute names reaching the DOM through serde no longer copy
  twice. Attribute names still cost one allocation each, because the flattening
  convention spells them `@name` and that byte has to be prepended somewhere;
  `Reader::read_value` has no such convention and stays free. The exact counts
  are pinned in `tests/zero_copy_tests.rs`.

### Removed

- **`StreamDeserializer`.** It took a `&[u8]`, so it could never stream, and
  being generic over `T: Deserialize` it forced every record to be decoded —
  which is the opposite of what a job forwarding rows wants. `Frames` and
  `FrameReader` replace it, yielding raw bytes that the caller decodes only if
  it needs to.

- **The compatibility modules** `yson_rs::{error, node, de, ser, attributes}`,
  which duplicated paths into `core` and `surface`. `Deserializer` and
  `Serializer` are re-exported at the crate root instead.

- **Three error variants that were never constructed**: `MalformedVarint`,
  `InvalidUtf8`, `UnexpectedToken`.

- **The `memchr` dependency**, which was declared and never used.

- `Writer::write_node`, `Serializer::into_output` (the `output` field is
  public), `YsonValue::with_attributes` and `YsonValue::as_list` — all with no
  callers and a one-line equivalent. `is_safe_unquoted` is now private.

- **The crate-local `[profile.release]`.** Cargo honours profiles only in the
  root package, so it was inert for anyone depending on this crate — and
  `panic = "abort"` is the wrong setting for a library in any case: it is not
  this crate's call to remove unwinding from a downstream binary.

### Testing

Test count went from 9 to 211. The suites that did not exist before:

- `tests/interop_tests.rs` — round trips against fixtures produced by the **Go**
  YSON implementation (`go.ytsaurus.tech/yt/go/yson`), vendored from
  [ss123she/yson-interop-tests](https://github.com/ss123she/yson-interop-tests).
  This is the only kind of test that catches being *self-consistently wrong*: a
  reader and writer that agree with each other and disagree with the cluster
  pass every round-trip test here and still break a job. It found the
  non-UTF-8 text output defect above.
- `tests/ytsaurus_protocol_tests.rs` — golden bytes for `table_index`,
  `row_index`, `range_index` and `key_switch` in both formats; a reduce input
  stream framed as a list fragment; the rule that an attributed *entity* is a
  control record and an attributed *map* is a data row, including for control
  records this version has never heard of; strings over 64 MiB; 10 000-column
  rows; malformed and deeply nested input.
- `tests/spec_conformance_tests.rs` — clause by clause against the YSON
  specification page, with a `known_deviations` module so every remaining
  disagreement is a live test rather than a claim.
- `tests/zero_copy_tests.rs` — allocation counts under a counting global
  allocator, so "zero copy" is measured rather than claimed.
- `tests/fuzz_smoke_tests.rs` — a seeded, deterministic no-panic sweep
  (truncation at every offset, single-bit corruption, random and
  structure-weighted bytes, splices, 5 000-deep nesting) that runs under
  `cargo test`, where `cargo fuzz` does not.
- `tests/container_tests.rs`, `tests/struct_shape_tests.rs`,
  `tests/exhaustion_tests.rs`, `tests/attributed_value_tests.rs`,
  `tests/dom_roundtrip_tests.rs` — one per defect above.

The `fuzz/` crate now builds. Its manifest depended on a package named `yson`,
which has never existed, so `cargo fuzz` could not have run against this crate
at any point.

Interop fixtures are no longer excluded from the published package: a crate that
ships its tests but not their inputs cannot be verified by the people depending
on it.

### Known limitations

- `<a=b>{}` reads back as `<a=b>#`. The flattening upstream of the DOM visitor
  consumes the empty `{}` without emitting a key, so by the time the stream
  arrives the two are identical. Distinguishing them means signalling the body
  kind out of `FlatStructAccess`, which would put a synthetic key in front of
  every typed struct as well.
- Maps round-trip as *values*, not byte for byte: `YsonNode::Map` is a
  `BTreeMap`, so keys come back in sorted order rather than in input order.
  `Row::raw`-style pass-through of the original bytes is the way to preserve
  order exactly.
- A missing `;` between items is accepted — `[1 2]` parses as `[1;2]`.
  `scan_value` deliberately matches the reader here, because a scanner stricter
  than the parser would frame records the parser then rejects.
- **MapFragment is unsupported.** The specification names three data types —
  Node, ListFragment and MapFragment — and only the first two have an entry
  point. `do = create; type = table` has no path through this crate.
- **An empty map key is accepted, and emitted.** The grammar forbids it.
  Refusing to *write* one needs an error channel the `Writer` token methods do
  not have.
- `FrameReader` rescans from the start of the pending record after each refill,
  so a single record much larger than the read buffer costs one rescan per
  refill. A resumable scanner would fix it.

### Migrating from 0.1

```rust
// The value tree carries a lifetime.
- fn handle(v: YsonValue) { .. }
+ fn handle(v: YsonValue<'_>) { .. }

// Fields that borrow must say so; serde only infers it for &str and &[u8].
  #[derive(Deserialize)]
  struct Row<'a> {
+     #[serde(borrow)]
      payload: YsonValue<'a>,
  }

// Detach a tree that has to outlive its buffer.
+ let owned: OwnedYsonValue = value.into_owned();

// Constructors take the format.
- Deserializer::from_bytes(bytes, true)
+ Deserializer::new(bytes, YsonFormat::Binary)
- Serializer::new(false)
+ Serializer::new(YsonFormat::Text)

// Module paths collapsed into the root.
- use yson_rs::{de::Deserializer, ser::Serializer, node::YsonValue};
+ use yson_rs::{Deserializer, Serializer, YsonValue};

// A sequence of values is framed, not deserialized in one call.
- let mut s = StreamDeserializer::<Row>::new(input, false);
- while let Some(row) = s.next_item()? { .. }
+ for frame in Frames::new(input, YsonFormat::Text) {
+     let row: Row = from_slice(frame?, YsonFormat::Text)?;
+ }
```

Two behaviour changes will surface as new errors rather than compile failures,
and both are the point of the change:

- `from_slice` now rejects trailing data. Code relying on `from_slice` reading a
  prefix of a longer buffer should use `Frames` or `scan_value`.
- A list longer than the fixed-length type it is read into is now an error
  rather than a silent truncation.

### Acknowledgements

Most of the defects fixed above were **not found here**. Eight of the ten were
found by [@sshaplygin](https://github.com/sshaplygin) while vendoring `ba2044c`
into [ytsaurus-rs](https://github.com/sshaplygin/ytsaurus-rs) as
`ytsaurus-yson`: three were reported as
[ss123she/yson-rs#1](https://github.com/ss123she/yson-rs/issues/1), and five
more were documented in that fork's own `CHANGELOG` as required by
Apache-2.0 §4(b).

Found there, fixed here:

| Defect | Where it was found |
| --- | --- |
| Stray `/` looped forever in `skip_ignored` | yson-rs#1 |
| Non-UTF-8 map keys rejected | yson-rs#1 |
| Non-UTF-8 attribute names silently renamed to `""` | yson-rs#1 |
| Three struct shapes serialized to invalid output | ytsaurus-yson CHANGELOG |
| `from_slice` ignored trailing data | ytsaurus-yson CHANGELOG |
| Ten-byte varint overflowed silently | ytsaurus-yson CHANGELOG |
| A container's terminator was left unread | ytsaurus-yson CHANGELOG |
| An attributed map lost its body | ytsaurus-yson CHANGELOG |

These were re-implemented from those written descriptions rather than ported,
and two go further than the reports: the container fix also covers
`Vec<(i32, i32)>` over `[[1;2];[3;4]]`, which silently returned one pair, and
the attributed-map fix makes the flattened reading total rather than adding a
case.

Four additions came from that fork as ideas: **`scan`** and its
`Complete`/`Incomplete` framing, **`Serialize` for the DOM**,
**`Serializer::with_buffer`**, and **`Copy` on `YsonFormat`**. So did the shape
of three test suites — golden protocol bytes, vendored Go fixtures, and a
seeded fuzz sweep that runs under `cargo test` where `cargo fuzz` cannot.

`FrameReader` carries one technique straight from their `ytsaurus-job`
`JobReader`: holding a *length* rather than a borrow between calls, so the
buffer is never frozen while a caller decides what to do with a record. That is
what makes peek-then-consume compose with a refilling buffer, and it is not
obvious.

Not from that fork, for the record: `Frames`/`FrameReader` as a protocol-free
layer in `core` (theirs is protocol-aware and lives in the job crate), the
borrowed value tree, the three short-read/malformed misclassifications, and the
two specification deviations — the last of which that fork still carries
unchanged.

The Go interop fixtures come from
[ss123she/yson-interop-tests](https://github.com/ss123she/yson-interop-tests);
vendoring them into the codec's own test suite is that fork's idea.

## [0.1.3] — `ba2044c`

The last release before the work above. Its history is in the git log; this
file starts here because 0.1.3 is the revision downstream forks were taken
from.
