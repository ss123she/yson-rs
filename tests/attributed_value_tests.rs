#![cfg(feature = "serde")]

//! Decoding an attributed value into [`YsonValue`] keeps its body.
//!
//! `<a=b>{x=10}` is the shape every attributed cluster response has.

use yson_rs::{Reader, Writer, YsonFormat, YsonMap, YsonNode, YsonValue, from_slice};

fn map_of<'a, 'v>(value: &'a YsonValue<'v>) -> &'a YsonMap<'v> {
    match &value.node {
        YsonNode::Map(m) => m,
        other => panic!("expected a map, got {other:?}"),
    }
}

fn attrs_of<'a, 'v>(value: &'a YsonValue<'v>) -> &'a YsonMap<'v> {
    value.attributes.as_ref().expect("expected attributes")
}

/// `<a="b">{x=10}` in binary YSON.
const ATTRIBUTED_MAP: &[u8] = &[
    b'<', 0x01, 0x02, b'a', b'=', 0x01, 0x02, b'b', b'>', b'{', 0x01, 0x02, b'x', b'=', 0x02, 0x14,
    b'}',
];

// --- The body survives ------------------------------------------------------

#[test]
fn an_attributed_map_keeps_its_body() {
    for (input, format) in [
        (&b"<a=b>{x=10}"[..], YsonFormat::Text),
        (ATTRIBUTED_MAP, YsonFormat::Binary),
    ] {
        let value: YsonValue = from_slice(input, format).unwrap();

        assert_eq!(
            attrs_of(&value)[b"a".as_slice()].as_bytes(),
            Some(&b"b"[..])
        );
        assert_eq!(map_of(&value)[b"x".as_slice()].as_i64(), Some(10));
    }
}

#[test]
fn an_attributed_map_keeps_every_key() {
    let value: YsonValue = from_slice(
        b"<schema=strict;user=root>{host=a;port=80;up=%true}",
        YsonFormat::Text,
    )
    .unwrap();

    let attrs = attrs_of(&value);
    assert_eq!(attrs.len(), 2);
    assert_eq!(attrs[b"schema".as_slice()].as_bytes(), Some(&b"strict"[..]));

    let body = map_of(&value);
    assert_eq!(body.len(), 3);
    assert_eq!(body[b"host".as_slice()].as_bytes(), Some(&b"a"[..]));
    assert_eq!(body[b"port".as_slice()].as_i64(), Some(80));
    assert_eq!(body[b"up".as_slice()].node, YsonNode::Boolean(true));
}

#[test]
fn a_nested_attributed_map_keeps_its_body() {
    let value: YsonValue = from_slice(b"{outer=<a=b>{x=10}}", YsonFormat::Text).unwrap();
    let inner = &map_of(&value)[b"outer".as_slice()];

    assert_eq!(attrs_of(inner)[b"a".as_slice()].as_bytes(), Some(&b"b"[..]));
    assert_eq!(map_of(inner)[b"x".as_slice()].as_i64(), Some(10));
}

#[test]
fn an_attributed_list_keeps_its_body() {
    let value: YsonValue = from_slice(b"<a=b>[1;2;3]", YsonFormat::Text).unwrap();

    assert_eq!(
        attrs_of(&value)[b"a".as_slice()].as_bytes(),
        Some(&b"b"[..])
    );
    match &value.node {
        YsonNode::List(items) => assert_eq!(items.len(), 3),
        other => panic!("expected a list, got {other:?}"),
    }
}

#[test]
fn the_round_trip_the_fix_exists_for() {
    // Read through the DOM and write back out: the bytes have to survive.
    let value: YsonValue = from_slice(b"<a=b>{x=10}", YsonFormat::Text).unwrap();

    let mut out = Vec::new();
    Writer::new(&mut out, YsonFormat::Text)
        .write_value(&value)
        .unwrap();

    assert_eq!(out, b"<a=b>{x=10}");
    assert_eq!(
        Reader::new(&out, YsonFormat::Text).read_value().unwrap(),
        value
    );
}

// --- The shapes that already worked -----------------------------------------

#[test]
fn an_attributed_scalar_still_reads() {
    let value: YsonValue = from_slice(b"<a=b>42", YsonFormat::Text).unwrap();
    assert_eq!(
        attrs_of(&value)[b"a".as_slice()].as_bytes(),
        Some(&b"b"[..])
    );
    assert_eq!(value.as_i64(), Some(42));
}

#[test]
fn an_attributed_entity_still_reads() {
    let value: YsonValue = from_slice(b"<a=b>#", YsonFormat::Text).unwrap();
    assert_eq!(
        attrs_of(&value)[b"a".as_slice()].as_bytes(),
        Some(&b"b"[..])
    );
    assert_eq!(value.node, YsonNode::Entity);
}

#[test]
fn a_plain_map_is_still_unattributed() {
    let value: YsonValue = from_slice(b"{x=10}", YsonFormat::Text).unwrap();
    assert!(value.attributes.is_none());
    assert_eq!(map_of(&value)[b"x".as_slice()].as_i64(), Some(10));
}

#[test]
fn an_empty_map_is_still_a_map() {
    let value: YsonValue = from_slice(b"{}", YsonFormat::Text).unwrap();
    assert!(value.attributes.is_none());
    assert_eq!(value.node, YsonNode::Map(YsonMap::new()));
}

// --- Two bodies for one value is an error -----------------------------------

#[test]
fn a_value_key_beside_plain_keys_is_an_error() {
    let err =
        from_slice::<YsonValue>(b"{\"@a\"=1;\"$value\"=2;x=3}", YsonFormat::Text).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("$value"), "unexpected error: {msg}");
    assert!(msg.contains('x'), "the offending key is not named: {msg}");
}

#[test]
fn a_value_key_alone_beside_attributes_is_still_fine() {
    let value: YsonValue = from_slice(b"{\"@a\"=1;\"$value\"=2}", YsonFormat::Text).unwrap();
    assert_eq!(attrs_of(&value)[b"a".as_slice()].as_i64(), Some(1));
    assert_eq!(value.as_i64(), Some(2));
}
