#![cfg(feature = "serde")]

//! The DOM encodes as well as decodes.
//!
//! A `YsonValue` nested in a struct has to be writable -- that is what a
//! pass-through job needs.

use serde::{Deserialize, Serialize};
use yson_rs::{
    Reader, Writer, YsonFormat, YsonMap, YsonNode, YsonValue, from_slice, to_string, to_vec,
};

/// Decode, encode, decode: the value has to survive both directions.
#[track_caller]
fn round_trips(text: &str) -> YsonValue<'_> {
    let value: YsonValue = from_slice(text.as_bytes(), YsonFormat::Text).unwrap();

    let re_text = to_string(&value, YsonFormat::Text).unwrap();
    let re_value: YsonValue = from_slice(re_text.as_bytes(), YsonFormat::Text).unwrap();
    assert_eq!(re_value, value, "text round trip changed the value");

    let binary = to_vec(&value, YsonFormat::Binary).unwrap();
    let from_binary: YsonValue = from_slice(&binary, YsonFormat::Binary).unwrap();
    assert_eq!(from_binary, value, "binary round trip changed the value");

    value
}

// --- Every node kind --------------------------------------------------------

#[test]
fn scalars_round_trip() {
    for text in [
        "#", "%true", "%false", "42", "-42", "42u", "1.5", "hello", "\"a b\"",
    ] {
        round_trips(text);
    }
}

#[test]
fn containers_round_trip() {
    for text in [
        "[]",
        "{}",
        "[1;2;3]",
        "{a=1;b=2}",
        "[[1;2];[3;4]]",
        "{a={b={c=1}}}",
        "[{a=1};{b=2}]",
    ] {
        round_trips(text);
    }
}

#[test]
fn attributed_values_round_trip() {
    for text in [
        "<a=b>42",
        "<a=b>#",
        "<a=b>{x=10}",
        "<a=b>[1;2]",
        "<a=1;b=2>{x=10;y=20}",
        "{outer=<a=b>{x=10}}",
        "[<a=b>1;<c=d>2]",
    ] {
        round_trips(text);
    }
}

#[test]
fn nested_attributes_round_trip() {
    let value = round_trips("<outer=<inner=1>2>3");
    assert_eq!(value.as_i64(), Some(3));
}

// --- Bytes that are not text ------------------------------------------------

#[test]
fn non_utf8_strings_keys_and_attribute_names_survive() {
    let mut original = Vec::new();
    let mut w = Writer::new(&mut original, YsonFormat::Binary);
    w.begin_attributes();
    w.write_string(b"\xff\xfe");
    w.key_value_separator();
    w.write_i64(1);
    w.end_attributes();
    w.begin_map();
    w.write_string(b"\x80key");
    w.key_value_separator();
    w.write_string(b"\xc3\x28");
    w.end_map();

    let value: YsonValue = from_slice(&original, YsonFormat::Binary).unwrap();

    // Binary is byte-for-byte, since there is one spelling of a string.
    let binary = to_vec(&value, YsonFormat::Binary).unwrap();
    assert_eq!(binary, original);

    // Text has to escape them, and read them back.
    let text = to_string(&value, YsonFormat::Text).unwrap();
    let from_text: YsonValue = from_slice(text.as_bytes(), YsonFormat::Text).unwrap();
    assert_eq!(from_text, value);
}

#[test]
fn utf8_strings_still_use_the_unquoted_form_where_they_can() {
    let value: YsonValue = from_slice(b"{host=name}", YsonFormat::Text).unwrap();
    assert_eq!(to_string(&value, YsonFormat::Text).unwrap(), "{host=name}");
}

// --- The DOM agrees with the format layer -----------------------------------

#[test]
fn serde_and_the_writer_emit_the_same_bytes() {
    for text in ["42", "[1;2]", "{a=1}", "<a=b>{x=10}", "<a=b>#", "[<k=1>2]"] {
        let value: YsonValue = from_slice(text.as_bytes(), YsonFormat::Text).unwrap();

        for format in [YsonFormat::Text, YsonFormat::Binary] {
            let mut direct = Vec::new();
            Writer::new(&mut direct, format)
                .write_value(&value)
                .unwrap();
            assert_eq!(
                to_vec(&value, format).unwrap(),
                direct,
                "serde and Writer disagree on {text:?} in {format:?}"
            );
        }
    }
}

// --- The pass-through case the impl exists for ------------------------------

#[test]
fn a_yson_value_nested_in_a_struct_serializes() {
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Row<'a> {
        id: i64,
        // `YsonValue` borrows from the input, and serde infers that only for
        // `&str`/`&[u8]`; every other borrowing field has to say so.
        #[serde(borrow)]
        payload: YsonValue<'a>,
    }

    let original = Row {
        id: 7,
        payload: from_slice(b"<a=b>{x=10}", YsonFormat::Text).unwrap(),
    };

    let text = to_string(&original, YsonFormat::Text).unwrap();
    assert_eq!(text, "{id=7;payload=<a=b>{x=10}}");
    assert_eq!(
        from_slice::<Row>(text.as_bytes(), YsonFormat::Text).unwrap(),
        original
    );
}

#[test]
fn a_node_serializes_without_its_value_wrapper() {
    let node = YsonNode::List(vec![
        YsonValue::new(YsonNode::Int64(1)),
        YsonValue::string(b"two".to_vec()),
    ]);
    assert_eq!(to_string(&node, YsonFormat::Text).unwrap(), "[1;two]");
}

#[test]
fn a_hand_built_tree_serializes() {
    let mut entries = YsonMap::new();
    entries.insert(b"n".to_vec().into(), YsonValue::new(YsonNode::Uint64(9)));

    let mut attributes = YsonMap::new();
    attributes.insert(
        b"kind".to_vec().into(),
        YsonValue::new(YsonNode::Boolean(true)),
    );

    let value = YsonValue {
        attributes: Some(attributes),
        node: YsonNode::Map(entries),
    };

    let text = to_string(&value, YsonFormat::Text).unwrap();
    assert_eq!(text, "<kind=%true>{n=9u}");
    assert_eq!(
        Reader::new(text.as_bytes(), YsonFormat::Text)
            .read_value()
            .unwrap(),
        value
    );
}
