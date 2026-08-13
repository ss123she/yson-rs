#![cfg(feature = "serde")]

//! `from_slice` reads the whole slice, not the front of it.

use yson_rs::{Scan, YsonFormat, YsonValue, from_slice, scan_value};

fn is_trailing(err: &yson_rs::YsonError) -> bool {
    err.to_string().contains("Trailing data")
}

// --- Trailing data is an error ----------------------------------------------

#[test]
fn a_second_value_is_refused() {
    let err = from_slice::<i64>(b"42 garbage", YsonFormat::Text).unwrap_err();
    assert!(is_trailing(&err), "unexpected error: {err}");
    assert!(err.to_string().contains('3'), "no offset named: {err}");
}

#[test]
fn a_concatenated_document_is_refused() {
    assert!(from_slice::<Vec<i32>>(b"[1;2][3;4]", YsonFormat::Text).is_err());
    assert!(from_slice::<YsonValue>(b"{a=1}{b=2}", YsonFormat::Text).is_err());
}

#[test]
fn a_stray_terminator_is_refused() {
    // The shape the container-closing fix used to leave behind.
    assert!(from_slice::<i64>(b"42]", YsonFormat::Text).is_err());
    assert!(from_slice::<i64>(b"42}", YsonFormat::Text).is_err());
}

#[test]
fn trailing_bytes_are_refused_in_binary_too() {
    // `42` followed by a byte that is not part of it.
    let input = &[0x02, 0x54, 0x02, 0x56][..];
    let err = from_slice::<i64>(input, YsonFormat::Binary).unwrap_err();
    assert!(is_trailing(&err), "unexpected error: {err}");
}

// --- Insignificant trailing bytes are not trailing data ----------------------

#[test]
fn trailing_whitespace_and_comments_are_fine() {
    for input in [
        &b"42 "[..],
        b"42\n\t ",
        b"42 // done",
        b"42 /* done */",
        b"42 /* unterminated",
    ] {
        assert_eq!(
            from_slice::<i64>(input, YsonFormat::Text).unwrap(),
            42,
            "for {input:?}"
        );
    }
}

#[test]
fn a_complete_value_is_still_accepted() {
    assert_eq!(
        from_slice::<Vec<i32>>(b"[1;2;3]", YsonFormat::Text).unwrap(),
        vec![1, 2, 3]
    );
    assert_eq!(
        from_slice::<(i32, i32)>(b"[1;2]", YsonFormat::Text).unwrap(),
        (1, 2)
    );
}

// --- A sequence of values is framed, not deserialized in one go -------------

#[test]
fn a_list_fragment_is_read_one_framed_value_at_a_time() {
    // `from_slice` refuses `1; 2; 3` on purpose. Framing it with `scan_value`
    // is the supported way through, and it is what a streaming reader does.
    assert!(from_slice::<i32>(b"1; 2; 3", YsonFormat::Text).is_err());

    let input: &[u8] = b"1; 2; 3";
    let mut rest = input;
    let mut values = Vec::new();

    while !rest.is_empty() {
        if rest[0] == b';' || rest[0].is_ascii_whitespace() {
            rest = &rest[1..];
            continue;
        }
        let Scan::Complete(len) = scan_value(rest, YsonFormat::Text).unwrap() else {
            panic!("the fragment is whole");
        };
        values.push(from_slice::<i32>(&rest[..len], YsonFormat::Text).unwrap());
        rest = &rest[len..];
    }

    assert_eq!(values, vec![1, 2, 3]);
}
