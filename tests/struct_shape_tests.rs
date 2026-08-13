#![cfg(feature = "serde")]

//! Every struct shape serializes to a valid YSON value, or to an error.
//!
//! A YSON value is optional `<attributes>` followed by exactly one body,
//! and the attributes stand strictly before what they decorate.

use serde::{Deserialize, Serialize};
use yson_rs::{YsonFormat, YsonValue, from_slice, to_string, to_vec};

/// Everything this crate emits has to be readable by the reader it ships with.
fn round_trips(text: &str) -> bool {
    from_slice::<YsonValue>(text.as_bytes(), YsonFormat::Text).is_ok()
}

// --- The shapes that used to produce invalid output --------------------------

#[test]
fn an_empty_struct_is_an_empty_map() {
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Empty {}

    let text = to_string(&Empty {}, YsonFormat::Text).unwrap();
    assert_eq!(text, "{}");
    assert!(round_trips(&text));
    assert_eq!(
        from_slice::<Empty>(text.as_bytes(), YsonFormat::Text).unwrap(),
        Empty {}
    );

    // Zero bytes is not a value in either format.
    assert!(!to_vec(&Empty {}, YsonFormat::Binary).unwrap().is_empty());
}

#[test]
fn an_all_attribute_struct_gets_a_value_node() {
    #[derive(Serialize)]
    struct AllAttrs {
        #[serde(rename = "@x")]
        x: i32,
    }

    let text = to_string(&AllAttrs { x: 1 }, YsonFormat::Text).unwrap();
    assert_eq!(text, "<x=1>#");
    assert!(round_trips(&text));

    let value: YsonValue = from_slice(text.as_bytes(), YsonFormat::Text).unwrap();
    assert_eq!(value.attr("x").unwrap().as_i64(), Some(1));
}

#[test]
fn an_attribute_after_a_plain_field_is_an_error() {
    #[derive(Serialize)]
    struct AttrAfterPlain {
        a: i32,
        #[serde(rename = "@x")]
        x: i32,
    }

    let err = to_string(&AttrAfterPlain { a: 1, x: 2 }, YsonFormat::Text).unwrap_err();
    assert!(
        err.to_string().contains("@x"),
        "the offending field is not named: {err}"
    );
}

#[test]
fn an_attribute_after_a_value_body_is_an_error() {
    #[derive(Serialize)]
    struct AttrAfterValue {
        #[serde(rename = "$value")]
        v: i32,
        #[serde(rename = "@x")]
        x: i32,
    }

    assert!(to_string(&AttrAfterValue { v: 1, x: 2 }, YsonFormat::Text).is_err());
}

#[test]
fn a_value_field_beside_plain_fields_is_an_error() {
    #[derive(Serialize)]
    struct ValueBesidePlain {
        a: i32,
        #[serde(rename = "$value")]
        v: i32,
    }

    let err = to_string(&ValueBesidePlain { a: 1, v: 2 }, YsonFormat::Text).unwrap_err();
    assert!(
        err.to_string().contains("$value"),
        "unexpected error: {err}"
    );
}

#[test]
fn a_plain_field_after_a_value_body_is_an_error() {
    #[derive(Serialize)]
    struct PlainAfterValue {
        #[serde(rename = "$value")]
        v: i32,
        a: i32,
    }

    let err = to_string(&PlainAfterValue { v: 1, a: 2 }, YsonFormat::Text).unwrap_err();
    assert!(err.to_string().contains('a'), "unexpected error: {err}");
}

// --- The shapes that were already right ------------------------------------

#[test]
fn a_plain_struct_is_still_a_map() {
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Plain {
        a: i32,
        b: i32,
    }

    let text = to_string(&Plain { a: 1, b: 2 }, YsonFormat::Text).unwrap();
    assert_eq!(text, "{a=1;b=2}");
    assert_eq!(
        from_slice::<Plain>(text.as_bytes(), YsonFormat::Text).unwrap(),
        Plain { a: 1, b: 2 }
    );
}

#[test]
fn attributes_before_a_map_body_still_work() {
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Decorated {
        #[serde(rename = "@x")]
        x: i32,
        a: i32,
        b: i32,
    }

    let text = to_string(&Decorated { x: 9, a: 1, b: 2 }, YsonFormat::Text).unwrap();
    assert_eq!(text, "<x=9>{a=1;b=2}");
    assert_eq!(
        from_slice::<Decorated>(text.as_bytes(), YsonFormat::Text).unwrap(),
        Decorated { x: 9, a: 1, b: 2 }
    );
}

#[test]
fn attributes_before_a_value_body_still_work() {
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Annotated {
        #[serde(rename = "@author")]
        author: String,
        #[serde(rename = "$value")]
        content: String,
    }

    let original = Annotated {
        author: "admin".into(),
        content: "hello".into(),
    };
    let text = to_string(&original, YsonFormat::Text).unwrap();
    assert_eq!(text, "<author=admin>hello");
    assert_eq!(
        from_slice::<Annotated>(text.as_bytes(), YsonFormat::Text).unwrap(),
        original
    );
}

#[test]
fn a_bare_value_body_still_works() {
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Wrapper {
        #[serde(rename = "$value")]
        v: i32,
    }

    let text = to_string(&Wrapper { v: 7 }, YsonFormat::Text).unwrap();
    assert_eq!(text, "7");
    assert_eq!(
        from_slice::<Wrapper>(text.as_bytes(), YsonFormat::Text).unwrap(),
        Wrapper { v: 7 }
    );
}

#[test]
fn every_valid_shape_round_trips_in_binary_too() {
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Empty {}
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Decorated {
        #[serde(rename = "@x")]
        x: i32,
        a: i32,
    }

    let empty = to_vec(&Empty {}, YsonFormat::Binary).unwrap();
    assert_eq!(
        from_slice::<Empty>(&empty, YsonFormat::Binary).unwrap(),
        Empty {}
    );

    let decorated = to_vec(&Decorated { x: 9, a: 1 }, YsonFormat::Binary).unwrap();
    assert_eq!(
        from_slice::<Decorated>(&decorated, YsonFormat::Binary).unwrap(),
        Decorated { x: 9, a: 1 }
    );
}

#[test]
fn nested_structs_still_nest() {
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Inner {
        #[serde(rename = "@k")]
        k: i32,
        v: i32,
    }
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Outer {
        inner: Inner,
        rest: i32,
    }

    let original = Outer {
        inner: Inner { k: 1, v: 2 },
        rest: 3,
    };
    let text = to_string(&original, YsonFormat::Text).unwrap();
    assert_eq!(text, "{inner=<k=1>{v=2};rest=3}");
    assert_eq!(
        from_slice::<Outer>(text.as_bytes(), YsonFormat::Text).unwrap(),
        original
    );
}
