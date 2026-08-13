#![cfg(feature = "serde")]

//! Regression tests for the three defects reported against v0.1.3 (`ba2044c`).
//!
//! Defect 1 (the stray-`/` hang) is a `core` concern and is pinned by the unit
//! tests in `src/core/reader.rs`; the two here are the serde-side ones.

use serde::Deserialize;
use yson_rs::{YsonFormat, YsonMap, YsonNode, YsonValue, from_slice};

/// `{"\xFF\xFE\x00" = 1}` in binary YSON.
const NON_UTF8_MAP_KEY: &[u8] = &[b'{', 0x01, 0x06, 0xFF, 0xFE, 0x00, b'=', 0x02, 0x02, b'}'];

/// `<"\xFF\xFE" = 1>#` in binary YSON.
const NON_UTF8_ATTR_NAME: &[u8] = &[b'<', 0x01, 0x04, 0xFF, 0xFE, b'=', 0x02, 0x02, b'>', b'#'];

fn map_of<'a, 'v>(value: &'a YsonValue<'v>) -> &'a YsonMap<'v> {
    match &value.node {
        YsonNode::Map(m) => m,
        other => panic!("expected a map, got {other:?}"),
    }
}

// --- Defect 2: non-UTF-8 map keys were rejected -----------------------------

#[test]
fn non_utf8_map_key_parses() {
    let value: YsonValue = from_slice(NON_UTF8_MAP_KEY, YsonFormat::Binary)
        .expect("a non-UTF-8 map key is a legal YSON document");

    let map = map_of(&value);
    assert_eq!(map.len(), 1);
    assert_eq!(map.keys().next().unwrap().as_ref(), &[0xFF, 0xFE, 0x00][..]);
    assert_eq!(map.values().next().unwrap().as_i64(), Some(1));
}

#[test]
fn utf8_map_keys_are_unaffected() {
    let value: YsonValue = from_slice(b"{host=1;port=2}", YsonFormat::Text).unwrap();
    let map = map_of(&value);
    assert_eq!(
        map.keys().map(|k| k.as_ref()).collect::<Vec<&[u8]>>(),
        vec![&b"host"[..], &b"port"[..]]
    );
}

#[test]
fn map_keys_with_mixed_encodings_stay_distinct() {
    // {"ok" = 1; "\xFF" = 2} in binary
    let bytes: &[u8] = &[
        b'{', 0x01, 0x04, b'o', b'k', b'=', 0x02, 0x02, b';', 0x01, 0x02, 0xFF, b'=', 0x02, 0x04,
        b'}',
    ];
    let value: YsonValue = from_slice(bytes, YsonFormat::Binary).unwrap();
    let map = map_of(&value);

    assert_eq!(map.len(), 2, "keys collided: {:?}", map.keys());
    assert_eq!(map[b"ok".as_slice()].as_i64(), Some(1));
    assert_eq!(map[[0xFFu8].as_slice()].as_i64(), Some(2));
}

// --- Defect 3: non-UTF-8 attribute names became "" ---------------------------

#[test]
fn non_utf8_attribute_name_is_preserved() {
    let value: YsonValue = from_slice(NON_UTF8_ATTR_NAME, YsonFormat::Binary).unwrap();

    let attributes = value.attributes.as_ref().expect("attributes are present");
    assert_eq!(attributes.len(), 1);
    assert_eq!(
        attributes.keys().next().unwrap().as_ref(),
        &[0xFF, 0xFE][..],
        "the attribute name was renamed rather than kept"
    );
    assert_eq!(value.attr_bytes(&[0xFF, 0xFE]).unwrap().as_i64(), Some(1));
}

#[test]
fn two_non_utf8_attribute_names_do_not_collide() {
    // <"\xFF" = 1; "\xFE" = 2># in binary — both used to become "".
    let bytes: &[u8] = &[
        b'<', 0x01, 0x02, 0xFF, b'=', 0x02, 0x02, b';', 0x01, 0x02, 0xFE, b'=', 0x02, 0x04, b'>',
        b'#',
    ];
    let value: YsonValue = from_slice(bytes, YsonFormat::Binary).unwrap();

    let attributes = value.attributes.as_ref().expect("attributes are present");
    assert_eq!(
        attributes.len(),
        2,
        "attribute names collided: {:?}",
        attributes.keys()
    );
    assert_eq!(value.attr_bytes(&[0xFF]).unwrap().as_i64(), Some(1));
    assert_eq!(value.attr_bytes(&[0xFE]).unwrap().as_i64(), Some(2));
}

#[test]
fn utf8_attribute_names_are_unaffected() {
    let value: YsonValue = from_slice(b"<author=admin;rows=2>#", YsonFormat::Text).unwrap();
    assert_eq!(value.attr("author").unwrap().as_str(), Some("admin"));
    assert_eq!(value.attr("rows").unwrap().as_i64(), Some(2));
}

/// The fix hands attribute names to the visitor as bytes. Derived field
/// identifiers implement `visit_bytes`, so a renamed field still matches — this
/// is what would break if the deserializer stopped being byte-oriented.
#[test]
fn renamed_struct_fields_still_match() {
    #[derive(Deserialize, PartialEq, Debug)]
    struct Table {
        #[serde(rename = "@row_count")]
        row_count: u64,
        path: String,
    }

    let parsed: Table =
        from_slice(b"<row_count=100>{path=\"/home/tables\"}", YsonFormat::Text).unwrap();

    assert_eq!(
        parsed,
        Table {
            row_count: 100,
            path: "/home/tables".to_string(),
        }
    );
}

/// A struct that also carries `$value` beside its attributes.
#[test]
fn attributed_value_struct_still_matches() {
    #[derive(Deserialize, PartialEq, Debug)]
    struct Annotated {
        #[serde(rename = "@author")]
        author: String,
        #[serde(rename = "$value")]
        content: String,
    }

    let parsed: Annotated = from_slice(b"<author=admin>\"hello\"", YsonFormat::Text).unwrap();
    assert_eq!(
        parsed,
        Annotated {
            author: "admin".to_string(),
            content: "hello".to_string(),
        }
    );
}

// --- The round trip both fixes exist for ------------------------------------

#[test]
fn non_utf8_names_survive_a_round_trip_through_the_writer() {
    use yson_rs::{Reader, Writer};

    let value: YsonValue = from_slice(NON_UTF8_ATTR_NAME, YsonFormat::Binary).unwrap();

    let mut out = Vec::new();
    Writer::new(&mut out, YsonFormat::Binary)
        .write_value(&value)
        .unwrap();

    let reparsed = Reader::new(&out, YsonFormat::Binary).read_value().unwrap();
    assert_eq!(reparsed, value);
    assert_eq!(
        reparsed.attr_bytes(&[0xFF, 0xFE]).unwrap().as_i64(),
        Some(1)
    );
}
