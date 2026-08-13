#![cfg(feature = "serde")]

//! The deserializer that opens a container closes it.
//!
//! A fixed-length visitor -- a tuple, a tuple struct, an array -- asks
//! exactly its length of times and stops, so nothing reads the terminator
//! unless the opener does.

use serde::Deserialize;
use yson_rs::{YsonFormat, from_slice};

fn both<T>(
    text: &str,
    binary: &[u8],
) -> (Result<T, yson_rs::YsonError>, Result<T, yson_rs::YsonError>)
where
    T: for<'de> Deserialize<'de>,
{
    (
        from_slice::<T>(text.as_bytes(), YsonFormat::Text),
        from_slice::<T>(binary, YsonFormat::Binary),
    )
}

/// `[[1;2];[3;4]]` in binary YSON.
const NESTED_PAIRS: &[u8] = &[
    b'[', b'[', 0x02, 0x02, b';', 0x02, 0x04, b']', b';', b'[', 0x02, 0x06, b';', 0x02, 0x08, b']',
    b']',
];

// --- The silent truncation --------------------------------------------------

#[test]
fn a_list_of_tuples_keeps_every_tuple() {
    let (text, binary) = both::<Vec<(i32, i32)>>("[[1;2];[3;4]]", NESTED_PAIRS);
    assert_eq!(text.unwrap(), vec![(1, 2), (3, 4)]);
    assert_eq!(binary.unwrap(), vec![(1, 2), (3, 4)]);
}

#[test]
fn a_list_of_arrays_keeps_every_array() {
    let (text, binary) = both::<Vec<[i32; 2]>>("[[1;2];[3;4]]", NESTED_PAIRS);
    assert_eq!(text.unwrap(), vec![[1, 2], [3, 4]]);
    assert_eq!(binary.unwrap(), vec![[1, 2], [3, 4]]);
}

#[test]
fn a_tuple_nested_in_a_tuple_reads() {
    let parsed: ((i32, i32), i32) = from_slice(b"[[1;2];3]", YsonFormat::Text).unwrap();
    assert_eq!(parsed, ((1, 2), 3));
}

#[test]
fn a_vec_nested_in_a_tuple_reads() {
    let parsed: (Vec<i32>, i32) = from_slice(b"[[1;2];3]", YsonFormat::Text).unwrap();
    assert_eq!(parsed, (vec![1, 2], 3));
}

#[test]
fn a_tuple_inside_a_map_does_not_end_the_map() {
    #[derive(Deserialize, PartialEq, Debug)]
    struct Holder {
        k: (i32, i32),
        j: i32,
    }

    let parsed: Holder = from_slice(b"{k=[1;2];j=9}", YsonFormat::Text).unwrap();
    assert_eq!(parsed, Holder { k: (1, 2), j: 9 });
}

#[test]
fn a_tuple_struct_consumes_its_own_bracket() {
    #[derive(Deserialize, PartialEq, Debug)]
    struct Pair(i32, i32);

    let parsed: Vec<Pair> = from_slice(b"[[1;2];[3;4]]", YsonFormat::Text).unwrap();
    assert_eq!(parsed, vec![Pair(1, 2), Pair(3, 4)]);
}

// --- A container longer than the visitor read is refused, not truncated ------

#[test]
fn a_list_longer_than_the_tuple_is_an_error() {
    let err = from_slice::<(i32, i32)>(b"[1;2;3]", YsonFormat::Text).unwrap_err();
    assert!(
        err.to_string().contains("close the container"),
        "unexpected error: {err}"
    );
}

#[test]
fn a_list_longer_than_the_array_is_an_error() {
    assert!(from_slice::<[i32; 2]>(b"[1;2;3]", YsonFormat::Text).is_err());
}

#[test]
fn a_list_shorter_than_the_tuple_is_still_an_error() {
    assert!(from_slice::<(i32, i32)>(b"[1]", YsonFormat::Text).is_err());
}

// --- The shapes that already worked keep working ----------------------------

#[test]
fn trailing_separators_are_still_allowed() {
    assert_eq!(
        from_slice::<(i32, i32)>(b"[1;2;]", YsonFormat::Text).unwrap(),
        (1, 2)
    );
    assert_eq!(
        from_slice::<Vec<i32>>(b"[1;2;]", YsonFormat::Text).unwrap(),
        vec![1, 2]
    );
    assert_eq!(
        from_slice::<Vec<i32>>(b"[]", YsonFormat::Text).unwrap(),
        Vec::<i32>::new()
    );
}

#[test]
fn deeply_nested_lists_still_close_in_order() {
    let parsed: Vec<Vec<Vec<i32>>> = from_slice(b"[[[1];[2]];[[3]]]", YsonFormat::Text).unwrap();
    assert_eq!(parsed, vec![vec![vec![1], vec![2]], vec![vec![3]]]);
}

#[test]
fn maps_nested_in_lists_still_close_in_order() {
    use std::collections::BTreeMap;

    let parsed: Vec<BTreeMap<String, i32>> =
        from_slice(b"[{a=1};{b=2}]", YsonFormat::Text).unwrap();
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0]["a"], 1);
    assert_eq!(parsed[1]["b"], 2);
}
