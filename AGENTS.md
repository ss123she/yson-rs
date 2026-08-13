# AGENTS.md

Context for coding agents working on **yson-rs**. Read this before changing
anything.

## What this is

A serde-based [YSON](https://ytsaurus.tech/docs/en/user-guide/storage/yson)
serializer and deserializer for Rust, text and binary. YSON is
[YTsaurus](https://ytsaurus.tech)' native format: MapReduce jobs read and write
it, and the HTTP API speaks it. This crate is the codec alone — no cluster
client, no job runtime.

Single crate, `yson-rs` 0.2.0, edition 2024, licensed **MIT OR Apache-2.0**
(both files are in the root and stay there).

## Layout

The crate is two layers, and **the boundary is the point**: `core` is the
format, `surface` is the serde binding. `lib.rs` re-exports the common types at
the root so the usual imports are one line.

| Path | What it is |
| --- | --- |
| `src/lib.rs` | The facade: `from_slice`, `to_vec`, `to_string`, and the root re-exports. |
| `src/core/` | **The format, no serde.** Builds with `default-features = false`. |
| `src/core/reader.rs` | `Reader`: bytes → `Token`s, and `read_value` → a `YsonValue` tree. Tokens borrow the input (`Cow`), which is what makes zero-copy decoding possible. |
| `src/core/writer.rs` | `Writer`: tokens and values → bytes. Borrows a `&mut Vec<u8>` rather than owning one, so several values share an allocation. Every string method takes `&[u8]`. |
| `src/core/node.rs` | The AST: `YsonValue` (attributes + node) and `YsonNode`. |
| `src/core/token.rs` | `Token`, the lexical units, and `TokenKind` — the same shapes with the payload dropped. |
| `src/core/format.rs` | `YsonFormat`. |
| `src/core/error.rs` | `YsonError`; its serde `Error` impls are feature-gated. |
| `src/core/varint.rs` | Varint and zigzag, for the binary format. |
| `src/core/scan.rs` | `scan_value`: bytes → the length of the first complete value. Builds nothing, allocates nothing. |
| `src/core/frames.rs` | `Frames` (over a slice) and `FrameReader<R: Read>` (over a pipe): a list fragment → one record's bytes at a time. |
| `src/surface/` | **The serde binding**, behind the `serde` feature (on by default). |
| `src/surface/de.rs` | `Deserializer`. |
| `src/surface/ser.rs` | `Serializer`, built on `core::Writer`. |
| `src/surface/access.rs` | serde `MapAccess`/`SeqAccess`/`EnumAccess` helpers — attribute wrapping, flat structs, `$value`. Private. |
| `src/surface/value.rs` | `Deserialize` **and `Serialize`** for `YsonValue`/`YsonNode`. |
| `src/surface/attributes.rs` | `WithAttributes<T, A>`, the typed way to carry `<…>` beside a value. |
| `benches/yson_benchmark.rs` | Criterion benchmark (`harness = false`, `required-features = ["serde"]`). |
| `tests/` | See *Testing*. All carry `#![cfg(feature = "serde")]`; `tests/data/` holds the Go interop fixtures and ships in the package. |
| `fuzz/` | `cargo-fuzz` targets: binary (`fuzz_target_1`) and text (`fuzz_target_2`). |

**Which layer does a change belong in?** If it is about what the bytes mean —
markers, escaping, quoting, varints, the value tree — it is `core`, and it takes
`&[u8]`. If it is about which YSON shape a Rust type takes — `@name`, `$value`,
how an enum is spelled — it is `surface`. A new `str` in `core` is almost always
a bug.

## Fixed decisions — do not revisit without a human

| Decision | Value |
| --- | --- |
| Crate name | **`yson-rs`**, published on crates.io by the author |
| Licence | **MIT OR Apache-2.0**, dual; keep `LICENSE-APACHE` and `LICENSE-MIT` unedited |
| Scope | codec only — text and binary YSON |
| Data model | strings, map keys and attribute names are **arbitrary byte strings**, `Vec<u8>`, not text |
| Layering | `core` (format) and `surface` (serde), one crate, `serde` feature on by default. **Not two crates** — the common case must stay one dependency line, and a split that has to be un-split later is worse than one deferred. |
| Public paths | **one spelling each.** The pre-split facade modules (`yson_rs::de`, `::ser`, `::node`, `::attributes`, `::error`) were removed in 0.2.0; everything is re-exported at the crate root or reached through `core`/`surface`. |
| Constructors | take a **`YsonFormat`**, never a `bool`. `new` is the essential constructor; `with_*` configures. |
| Release profile | **none in this crate.** Cargo honours profiles only in the root package, so one here is inert for dependents — and `panic = "abort"` is not a library's call. `[profile.bench]` stays, because benchmarking runs with this crate as root. |

## Hard rules

1. **Publish nothing to crates.io** without explicit human approval.
2. **Strings are bytes.** YTsaurus column values, map keys and attribute names
   are arbitrary byte strings. Any code path that rounds one through `str` or
   `String` is a bug unless it is a documented, opt-in convenience with a
   `_bytes` sibling — see *The pattern*, rule 6.
3. Format facts are verified against the official YTsaurus documentation, not
   against this crate's own tests. If code and docs disagree, **re-read the docs
   first**, then change the code. Cite the doc at the point of use.
4. Every change ends green: `cargo fmt --check`, `cargo clippy --all-targets --
   -D warnings`, `cargo test`. All three pass today, so a failure is yours.
5. **No scope creep.** A cluster client, a job runtime, Skiff, protobuf rows and
   RPC-proxy support are out of scope until a human decides otherwise.

## Commands

```sh
cargo test                        # 236 tests: 207 unit and integration, 29 doc
cargo clippy --all-targets -- -D warnings
cargo fmt --all

# The codec alone. Both of these have to stay green, or the split is a fiction.
cargo test --no-default-features
cargo clippy --no-default-features --all-targets -- -D warnings

cargo bench                       # criterion, benches/yson_benchmark.rs

cargo +nightly fuzz run fuzz_target_1   # binary input
cargo +nightly fuzz run fuzz_target_2   # text input
```

`--no-default-features` runs the `core` unit tests plus the doctests that do
not need serde. The integration tests are all serde tests and gate
themselves out. The crate-level "typed" example is behind a
`#![cfg_attr(feature = "serde", doc = …)]` for the same reason — a doctest in
crate docs is compiled whatever the feature set, so it cannot just be written
inline.

`fuzz/Cargo.toml` used to name a package (`yson`) that has never existed, so
`cargo fuzz` could not have run against this crate at any point in its history.
Fixed in 0.2.0; the crate builds now.

## Format reference

### Binary YSON markers

| Marker | Type | Payload |
| --- | --- | --- |
| `0x01` | string | zigzag varint length (`sint32`), then that many raw bytes |
| `0x02` | int64 | zigzag varint (`sint64`) |
| `0x03` | double | 8 bytes little-endian |
| `0x04` / `0x05` | boolean | false / true |
| `0x06` | uint64 | unsigned varint |
| `0x23` `#` | entity | — |
| `< >` `[ ]` `{ }` `=` `;` | attributes, list, map, key/value separator, item separator | literal ASCII |

The structural bytes are the same ASCII in both formats; only the scalars change.

### Text quoting

`ser::is_safe_unquoted` writes a string bare when the first byte is a letter or
`_` and the rest are alphanumeric or `_-.`; everything else is quoted. So the
same value can render either way depending on its first byte. **Never assert on
the rendered text of a generated value** — compare decoded values instead. Both
spellings are valid YSON and YTsaurus accepts either.

### Attributes and `$value`

`<a=1>{x=2}` is attributes on a map. In serde terms a field renamed `@a` is an
attribute and a field renamed `$value` is the body, which is how a primitive
carries attributes at all (`<author=admin>"hello"`). `WithAttributes<T, A>` is
the typed form.

## The pattern: **a borrowed pipeline**

There is one architectural idea in this crate, and every rule below falls out of
it. Name it when you review a change; a change that does not fit it is a change
that needs an argument.

> **Each layer is a view over the caller's bytes that adds structure without
> taking ownership, and a caller may stop at any layer.**

```
bytes  ──▶  Frames / FrameReader  ──▶  Reader  ──▶  YsonValue<'a>  ──▶  serde
            (record boundaries)      (tokens)      (tree)            (Rust types)
              stop here to               stop here to      stop here to
              forward a row             skip a column     work untyped
```

`scan_value` is the same pipeline with every stage after boundary-finding
removed — which is why it allocates nothing at all.

### The eight rules it generates

1. **A caller may stop at any layer.** Adding a layer is fine; making a lower
   layer depend on a higher one is not. `core` never mentions `surface`.
2. **Nothing copies the caller's bytes.** Copying is a named, explicit
   operation: `into_owned`. The two places that must allocate — a text string
   carrying escapes, a value built by hand — are documented as exceptions and
   measured in `tests/zero_copy_tests.rs`.
3. **Whoever writes an opening token writes the closing one.** Do not trust a
   visitor to consume a terminator; fixed-length visitors do not. This is
   `Deserializer::close_container`.
4. **"Need more bytes" is a different answer from "these bytes are wrong."**
   `Eof`/`UnexpectedEof` mean short read; everything else means malformed.
   `scan` and `frames` route on that distinction, so a new parse path that
   reports a truncation as `Custom` silently breaks streaming. Three bugs have
   come from exactly this.
5. **Across a refill boundary, carry lengths, not borrows.** `FrameReader`
   stores a length between calls so the buffer is never frozen while a caller
   decides what to do with a record.
6. **Bytes, not `str`.** A `&str` overload in `core` is acceptable only as a
   convenience with a `_bytes` sibling beside it (`get`/`get_bytes`,
   `attr`/`attr_bytes`, `as_str`/`as_bytes`). Anything else is rule 2 of *Hard
   rules*.
7. **Limits belong to the caller**, with a documented default:
   `with_max_depth`, `with_max_record_bytes`, `read_value_with_max_depth`.
   Untrusted input is the normal case here, not the exotic one.
8. **Make the illegal state unrepresentable where it is cheap.** `StructState`
   replaced two booleans that could encode a shape YSON has no spelling for;
   `TokenKind` is `Token` with the payload removed so a scanner cannot allocate
   one by accident.

### Where the pattern is currently bent

Three known deviations. Each is small, none is worth a breaking change on its
own, and all three should be swept together when something else forces one.

- **`FrameReader` is the only thing in `core` that touches `std::io`.** Framing
  is format knowledge, so it belongs here; the `R: Read` bound is the exception
  that proves it. If `core` ever wants `no_std`, this is the line to cut.
- **`YsonError::Custom` renders as `"Custom error from serde: {0}"`** even when
  the `serde` feature is off and the error came from the reader. Surface
  vocabulary leaking down a layer.
- **`with_` means two things.** `Serializer::with_buffer` is a constructor;
  `with_max_depth` and friends are consuming builders. Same prefix, different
  contracts.

## Architecture notes

- **`YsonNode::Map` is a `BTreeMap`, so decode-then-re-encode sorts the keys**
  and is *not* byte-exact. Anything that must reproduce input byte-for-byte has
  to keep the original bytes. `Writer::write_value` says so in its own docs.
- **`Serializer` still owns `pub output: Vec<u8>`, deliberately.** The bench and
  three test files both borrow it (`&ser.output`) and move out of it
  (`let bytes = ser.output;`), and a move cannot go through a `Deref`. So the
  serde `Serializer` keeps the buffer and builds a transient `Writer` over it
  per call; `Writer` borrows `&mut Vec<u8>` rather than owning one. That is why
  the extraction cost no API break.
- **`Writer::write_value` and `Reader::read_value` are fallible, the primitive
  methods are not.** Writing an `i64` into a `Vec` cannot fail; walking an
  arbitrarily deep tree can, and both directions carry the same
  `DEFAULT_MAX_DEPTH` of 128 that the deserializer uses. A recursion guard on
  only one side would be a stack overflow on the other.
- `Token::String` is a `Cow<[u8]>`: borrowed when the input has no escapes,
  owned when it does. Keeping that split is what the benchmark measures.
- `to_string` refuses `YsonFormat::Binary` outright — binary YSON is not UTF-8.
- The deserializer decodes a string as `str` when it is valid UTF-8 and as bytes
  otherwise, so a non-UTF-8 column reaches a `String` field as an error and a
  `serde_bytes` field as data.

## Fixed defects — keep them fixed

`CHANGELOG.md` is the record: **ten** defects fixed since `ba2044c`, each with a
named regression test. Do not reintroduce any of them. **Eight of the ten were
found by [@sshaplygin](https://github.com/sshaplygin)** while vendoring this
crate downstream — see *Acknowledgements* in `CHANGELOG.md` for which, and for
what else came from that fork. Credit them in any summary of this work. Five caused silent data
loss or invalid output rather than an error, and those are the ones worth
knowing by heart:

- A container's terminator was the visitor's job, so `[[1;2];[3;4]]` into
  `Vec<(i32, i32)>` returned one pair (rule 3 above).
- An attributed map lost its body: `<a=b>{x=10}` decoded to an attributed
  entity.
- `from_slice` read the front of a document and called it the whole.
- Three struct shapes serialized to output this crate's own parser rejects.
- `\a \b \f \v` decoded to the letter, so a backspace became a `b`.

Three more were short-read-vs-malformed confusions (rule 4): a varint, a `%`
special value, and a text scalar at a buffer boundary. The three below are the
originals reported downstream, kept in full because the *why* still matters.

1. **Infinite loop on a stray `/` in text input.** `skip_ignored` treated `/` as
   the start of a comment; a `/` followed by anything other than `/` or `*`
   matched neither branch, advanced nothing, and `continue`d at the same
   position. `/a` never returned. It allocates nothing, so it presented as a
   *hung process* rather than an OOM — a two-byte denial of service for any
   caller parsing untrusted text.
   **Fix** (`core/reader.rs`): a `/` that opens no comment `break`s out of
   `skip_ignored` and is left for the tokenizer to reject as an invalid marker.
   An unterminated `/*` now runs `pos` to the end of the input instead of
   stopping at `len - 1` and handing the tokenizer a stray byte.
   **Tests**: `stray_slash_is_refused_not_looped`,
   `unterminated_block_comment_reaches_eof`, `comments_still_work`,
   `a_slash_inside_a_string_is_untouched` in `core/reader.rs`. These *hang* on
   the unfixed reader rather than failing, so a timeout in that module is the
   regression signal — not an assertion failure.
2. **Non-UTF-8 map keys were rejected.** `Deserialize for YsonValue` read keys
   as `map.next_key::<String>()`, so a legal document failed to parse even
   though `YsonNode::Map` stores `Vec<u8>`.
   **Fix** (`surface/value.rs`): a private `MapKey(Vec<u8>)` newtype whose
   visitor implements `visit_str`, `visit_string`, `visit_bytes` and
   `visit_byte_buf`. The reader hands back whichever spelling the input allows,
   so all four are needed. No public API change.
3. **Non-UTF-8 attribute names were silently replaced with `""`** — a literal
   `str::from_utf8(..).unwrap_or("")`, which lost the name and collapsed two
   such attributes into one. The most dangerous of the three, because it failed
   *quietly*.
   **Fix** (`surface/access.rs`): the `@`-prefixed name is built as bytes and
   handed to the visitor through a `ByteKeyDeserializer` that calls
   `visit_bytes` with no UTF-8 validation.
   **Why `#[serde(rename = "@name")]` still works**: `#[derive(Deserialize)]`
   generates a `visit_bytes` arm for field identifiers alongside `visit_str`.
   That is load-bearing and not obvious — `renamed_struct_fields_still_match`
   and `attributed_value_struct_still_matches` exist to catch it if it ever
   stops being true.

Tests 2 and 3 live in `tests/regression_tests.rs`, using the exact byte arrays
from the report. Each pairs the fixed case with an unaffected UTF-8 case and a
collision case, because "the name survived" and "two names stayed distinct" are
different claims.

## Testing

207 unit and integration tests, 29 doc tests. The ones that carry weight:

| Suite | What it is for |
| --- | --- |
| `tests/interop_tests.rs` | Bytes produced by the **Go** implementation. The only suite that catches being *self-consistently wrong* — a reader and writer that agree with each other and disagree with the cluster pass everything else and still break a job. It found the non-UTF-8 text output defect. |
| `tests/spec_conformance_tests.rs` | Clause by clause against the YSON page, including a `known_deviations` module so every deviation is a live test rather than a claim. |
| `tests/ytsaurus_protocol_tests.rs` | Control records, reduce streams, >64 MiB strings, 10 000-column rows. |
| `tests/zero_copy_tests.rs` | Allocation counts under a counting global allocator. "Zero copy" is measured, not asserted. |
| `tests/fuzz_smoke_tests.rs` | Seeded, deterministic no-panic sweep that runs under `cargo test`, where `cargo fuzz` does not. Fixed seed on purpose: a test that fails one run in fifty is a test nobody trusts. |
| `core/reader.rs`, `core/frames.rs` | Format-layer unit tests; these run with the feature off. |

Two habits that have each caught a real bug this crate shipped:

- **Feed a reader one byte at a time.** `FrameReader`'s trickle tests found two
  defects that were unreachable through the slice API.
- **Prefer a fixture from a real cluster or another implementation over a
  hand-built one.** This crate's own reading of the spec, checked against
  itself, proves nothing.

## Known consumers

[sshaplygin/ytsaurus-rs](https://github.com/sshaplygin/ytsaurus-rs) vendors this
crate as `ytsaurus-yson`, pinned at `ba2044c` (v0.1.3), under Apache-2.0 with a
`NOTICE` and a `CHANGELOG` of modifications.

Everything that fork added is now upstream here — its seven fixes, its `scan`
module, `Serialize` for the DOM, buffer reuse — and this crate has since gone
further: the borrowed DOM, `frames`, and three defects the fork still carries
unchanged. Its four spec deviations (see *Spec conformance*) are byte-identical
to the code they vendored, so fixing one here fixes it for them too.

`Frames`/`FrameReader` is **not** from that fork — theirs is protocol-aware and
lives in the job crate — but `FrameReader` takes one technique from their
`JobReader`: hold a length, not a borrow, between calls.

Their `ytsaurus-job/src/reader.rs` is the reference for what a job harness needs
from this crate. Read it before changing `frames` or `scan`. Note what it does
**not** need from us: control records, key switches and row indices are its
business, not ours.

## Spec conformance

Checked clause by clause against the page in *Reference*;
`tests/spec_conformance_tests.rs` is that check, and its `known_deviations`
module keeps each deviation a live test rather than a claim.

Conformant: all three string forms and every C escape, both int64 and uint64
text forms, all five double spellings, booleans, entity, every binary marker,
attributes on every literal kind including nested, list fragments with the
optional trailing `;`, whitespace anywhere. Every worked example on the page
parses.

Four known deviations, in the order they should be fixed:

1. **MapFragment is unsupported.** The page names three data types — Node,
   ListFragment, MapFragment — and only the first two have an entry point.
   `do = create; type = table` has no path through this crate. Needs a
   `MapFragments` iterator beside `Frames`.
2. **An empty key is accepted, and emitted.** The grammar says *"Key cannot be
   empty"*. Reading one leniently is defensible; `write_string(b"")` producing
   `{""=1}` is not. Fixing the write side needs an error channel the `Writer`
   token methods do not currently have.
3. **Comments are accepted** (`//`, `/* */`) though the grammar has none — `/`
   is a YPath token, not a YSON one. Leniency on input, so it cannot break
   reading valid YSON.
4. **A missing separator is accepted**: `[1 2]` parses as `[1;2]`. `scan_value`
   matches the reader here deliberately — a scanner stricter than the parser
   would frame records the parser then rejects.

The page contradicts itself once: its attribute example is
`<a = 10; b = [7,7,8]>"some-string"`, with commas inside the list, while the
text states semicolons replace commas. This crate rejects the commas. If a real
cluster ever emits them, that assumption is the thing to revisit.

## Reference

[YSON](https://ytsaurus.tech/docs/en/user-guide/storage/yson) ·
[interop-tests](https://github.com/ss123she/yson-interop-tests) ·
[serde](https://serde.rs/)
