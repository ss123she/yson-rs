#![cfg(feature = "serde")]

//! A conformance corpus: bytes produced by the **Go** YSON implementation.
//!
//! This is a *snapshot*, not a live cross-implementation check. The live check
//! needs Go installed and lives in
//! [ss123she/yson-interop-tests](https://github.com/ss123she/yson-interop-tests);
//! these fixtures are its output, vendored so that `cargo test` alone can prove
//! this crate reads what the reference implementation writes.
//!
//! It is the only test here that catches being *self-consistently wrong* — a
//! reader and writer that agree with each other and disagree with the cluster
//! pass every round trip in this repository and still break a job.
//!
//! Not asserted: byte equality with Go's output. Go writes floats as `1.100000`
//! where this crate writes `1.1`, and both are valid YSON for the same `f64`.
//! Conformance is agreement about *values*, not about spelling.
//!
//! ## Regenerating
//!
//! ```text
//! git clone https://github.com/ss123she/yson-interop-tests
//! cd yson-interop-tests && just test-all      # needs Go, Rust and just
//! cp data/*.bin data/*.txt <this repo>/tests/data/
//! ```
//!
//! Fixtures were taken at `yson-interop-tests@HEAD`, generated with
//! `go.ytsaurus.tech/yt/go/yson`. If the generator changes what it writes, both
//! the bytes and the expectations below have to be updated together.

use yson_rs::{Reader, YsonFormat, YsonNode, YsonValue};

const GO_BINARY: &[u8] = include_bytes!("data/go_to_rust_binary.bin");
const GO_TEXT: &[u8] = include_bytes!("data/go_to_rust_text.txt");
const RUST_BINARY: &[u8] = include_bytes!("data/rust_to_go_binary.bin");
const RUST_TEXT: &[u8] = include_bytes!("data/rust_to_go_text.txt");

fn read(input: &[u8], format: YsonFormat) -> YsonValue<'_> {
    Reader::new(input, format)
        .read_value()
        .unwrap_or_else(|e| panic!("Go fixture did not parse in {format:?}: {e}"))
}

#[track_caller]
fn field<'a>(doc: &'a YsonValue<'a>, name: &str) -> &'a YsonValue<'a> {
    doc.get(name)
        .unwrap_or_else(|| panic!("fixture has no field {name:?}"))
}

// --- The values Go wrote -----------------------------------------------------

/// Runs every assertion against one parsed fixture, so text and binary are held
/// to exactly the same standard.
fn assert_go_document(doc: &YsonValue<'_>) {
    // Integers at the limits. Go decremented the two maxima before writing.
    assert_eq!(field(doc, "int_min").as_i64(), Some(i64::MIN));
    assert_eq!(field(doc, "int_max").as_i64(), Some(i64::MAX - 1));
    assert_eq!(field(doc, "uint_max").as_u64(), Some(u64::MAX - 1));
    assert_eq!(field(doc, "int_zero").as_i64(), Some(0));
    assert_eq!(field(doc, "int_neg_one").as_i64(), Some(-1));

    // Floats, including the three spellings that are not numbers.
    assert!(field(doc, "float_nan").as_f64().unwrap().is_nan());
    assert_eq!(field(doc, "float_inf").as_f64(), Some(f64::INFINITY));
    assert_eq!(
        field(doc, "float_neg_inf").as_f64(),
        Some(f64::NEG_INFINITY)
    );
    assert_eq!(field(doc, "float_zero").as_f64(), Some(0.0));

    // Strings, and the escapes Go chose to write them with.
    assert_eq!(field(doc, "empty_str").as_bytes(), Some(&b""[..]));
    assert_eq!(
        field(doc, "special_str").as_str(),
        Some("Line1\nLine2\t\0\"\\_modified")
    );

    // The byte array is the sharpest case in the corpus: Go spells it as raw
    // UTF-8 (`\xDE\xAD`), two hex escapes, a one-digit octal escape (`\0`), a
    // three-digit octal escape (`\377`) and a bare ASCII byte -- four escape
    // forms in one literal.
    assert_eq!(
        field(doc, "byte_array").as_bytes(),
        Some(&[0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0xFF, 0x42][..])
    );

    // Option: `Some` is the value, `None` is an entity.
    assert_eq!(field(doc, "some_val").as_str(), Some("Present_modified"));
    assert_eq!(field(doc, "none_val").node, YsonNode::Entity);

    // Nested collections, and the trailing separators Go emits inside them.
    let nested = field(doc, "nested_list");
    let YsonNode::List(outer) = &nested.node else {
        panic!("nested_list is not a list");
    };
    assert_eq!(outer.len(), 3);
    assert_eq!(outer[0].node, YsonNode::List(Vec::new()));
    let YsonNode::List(second) = &outer[1].node else {
        panic!("nested_list[1] is not a list");
    };
    let second: Vec<i64> = second.iter().map(|v| v.as_i64().unwrap()).collect();
    assert_eq!(second, vec![1, 2, 3, 4]);
    let YsonNode::List(third) = &outer[2].node else {
        panic!("nested_list[2] is not a list");
    };
    assert_eq!(third[0].as_i64(), Some(-100));

    assert!(
        field(doc, "empty_map")
            .as_map()
            .expect("empty_map is a map")
            .is_empty()
    );

    // Attributes on a string, and on a list.
    let attributed_str = field(doc, "attributed_str");
    assert_eq!(
        attributed_str
            .attr("description")
            .and_then(YsonValue::as_str),
        Some("Just a string")
    );
    assert_eq!(
        attributed_str.attr("timestamp").and_then(YsonValue::as_u64),
        Some(999_999)
    );
    assert_eq!(
        attributed_str.as_str(),
        Some("Hello with attributes_from_go")
    );

    let attributed_list = field(doc, "attributed_list");
    assert_eq!(
        attributed_list.attr("list_id").and_then(YsonValue::as_str),
        Some("list-x")
    );
    let YsonNode::List(items) = &attributed_list.node else {
        panic!("attributed_list is not a list");
    };
    let items: Vec<f64> = items.iter().map(|v| v.as_f64().unwrap()).collect();
    assert_eq!(items, vec![1.1, 2.2]);
}

#[test]
fn go_binary_output_decodes() {
    assert_go_document(&read(GO_BINARY, YsonFormat::Binary));
}

#[test]
fn go_text_output_decodes() {
    assert_go_document(&read(GO_TEXT, YsonFormat::Text));
}

#[test]
fn the_two_go_formats_carry_the_same_document() {
    // Not a byte comparison: the same value, spelled two ways by the same
    // implementation, has to arrive here as the same value.
    let binary = read(GO_BINARY, YsonFormat::Binary);
    let text = read(GO_TEXT, YsonFormat::Text);

    let YsonNode::Map(binary_fields) = &binary.node else {
        panic!("expected a map");
    };
    let YsonNode::Map(text_fields) = &text.node else {
        panic!("expected a map");
    };
    assert_eq!(
        binary_fields.keys().collect::<Vec<_>>(),
        text_fields.keys().collect::<Vec<_>>()
    );

    for (key, binary_value) in binary_fields {
        let text_value = &text_fields[key];
        // NaN is not equal to itself, so it is checked structurally instead.
        if matches!(binary_value.node, YsonNode::Double(d) if d.is_nan()) {
            assert!(matches!(text_value.node, YsonNode::Double(d) if d.is_nan()));
            continue;
        }
        assert_eq!(
            binary_value,
            text_value,
            "field {} differs between formats",
            String::from_utf8_lossy(key)
        );
    }
}

// --- The bytes Go accepted from Rust ----------------------------------------

#[test]
fn the_rust_side_of_the_corpus_still_reads() {
    // These were produced by this crate and verified by the Go reader. If a
    // change here stops them parsing, the change broke something Go accepted.
    for (fixture, format) in [
        (RUST_BINARY, YsonFormat::Binary),
        (RUST_TEXT, YsonFormat::Text),
    ] {
        let doc = read(fixture, format);
        assert!(doc.as_map().is_some(), "{format:?} fixture is not a map");
        assert_eq!(
            doc.get("int_min").and_then(YsonValue::as_i64),
            Some(i64::MIN)
        );
        assert_eq!(
            doc.get("int_max").and_then(YsonValue::as_i64),
            Some(i64::MAX)
        );
        assert_eq!(
            doc.get("uint_max").and_then(YsonValue::as_u64),
            Some(u64::MAX)
        );
    }
}

// --- What we write, we can read ---------------------------------------------

#[test]
fn re_encoding_a_go_document_preserves_it() {
    use yson_rs::{Writer, to_vec};

    let original = read(GO_BINARY, YsonFormat::Binary);

    for format in [YsonFormat::Text, YsonFormat::Binary] {
        let mut bytes = Vec::new();
        Writer::new(&mut bytes, format)
            .write_value(&original)
            .unwrap();
        assert_go_document(&read(&bytes, format));

        // And through serde, which must agree with the writer.
        let via_serde = to_vec(&original, format).unwrap();
        assert_eq!(via_serde, bytes, "serde and Writer disagree in {format:?}");
    }
}
