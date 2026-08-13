#![cfg(feature = "serde")]

//! Clause-by-clause checks against the YSON specification.
//!
//! <https://ytsaurus.tech/docs/en/user-guide/storage/yson>
//!
//! Where this crate knowingly deviates, the deviation is named at the bottom
//! rather than left for someone to discover.

use yson_rs::{Frames, Reader, YsonFormat, YsonNode, YsonValue};

fn read(input: &[u8]) -> Result<YsonValue<'_>, yson_rs::YsonError> {
    Reader::new(input, YsonFormat::Text).read_value()
}

#[track_caller]
fn bytes_of(input: &str) -> Vec<u8> {
    read(input.as_bytes())
        .unwrap_or_else(|e| panic!("{input} did not parse: {e}"))
        .as_bytes()
        .expect("a string")
        .to_vec()
}

// --- Strings: identifiers, text, binary --------------------------------------

#[test]
fn identifiers_match_the_documented_pattern() {
    // [A-Za-z_][A-Za-z0-9_.\-]*, with the page's own examples.
    for id in ["abc123", "_", "a-b", "a.b", "_x.y-z9", "Z9"] {
        assert_eq!(bytes_of(id), id.as_bytes(), "identifier {id}");
    }
    // A leading digit or `-` is a number, not an identifier.
    assert_ne!(read(b"9lives").unwrap().as_bytes(), Some(&b"9lives"[..]));
}

#[test]
fn text_strings_decode_every_c_escape() {
    assert_eq!(bytes_of(r#""\a""#), [0x07]);
    assert_eq!(bytes_of(r#""\b""#), [0x08]);
    assert_eq!(bytes_of(r#""\f""#), [0x0C]);
    assert_eq!(bytes_of(r#""\n""#), [0x0A]);
    assert_eq!(bytes_of(r#""\r""#), [0x0D]);
    assert_eq!(bytes_of(r#""\t""#), [0x09]);
    assert_eq!(bytes_of(r#""\v""#), [0x0B]);
    assert_eq!(bytes_of(r#""\\""#), [0x5C]);
    assert_eq!(bytes_of(r#""\"""#), [0x22]);
    assert_eq!(bytes_of(r#""\'""#), [0x27]);
    assert_eq!(bytes_of(r#""\?""#), [0x3F]);
    // Hex and octal.
    assert_eq!(bytes_of(r#""\xEA""#), [0xEA]);
    assert_eq!(bytes_of(r#""\0""#), [0x00]);
    assert_eq!(bytes_of(r#""\377""#), [0xFF]);

    // The page's own example.
    assert_eq!(
        bytes_of(r#""quotation-mark: \", backslash: \\, tab: \t, unicode: \xEA""#),
        b"quotation-mark: \", backslash: \\, tab: \t, unicode: \xEA"
    );
}

#[test]
fn every_escape_survives_a_round_trip() {
    use yson_rs::{Writer, to_vec};

    let all: Vec<u8> = (0u8..=255).collect();
    for format in [YsonFormat::Text, YsonFormat::Binary] {
        let mut bytes = Vec::new();
        Writer::new(&mut bytes, format).write_string(&all);
        assert_eq!(
            Reader::new(&bytes, format).read_value().unwrap().as_bytes(),
            Some(&all[..]),
            "{format:?}"
        );
        assert_eq!(to_vec(&YsonValue::string(&all[..]), format).unwrap(), bytes);
    }
}

#[test]
fn binary_strings_use_the_documented_marker() {
    // \x01 + length (zigzag varint) + data
    let input: &[u8] = &[0x01, 0x06, b'a', b'b', b'c'];
    assert_eq!(
        Reader::new(input, YsonFormat::Binary)
            .read_value()
            .unwrap()
            .as_bytes(),
        Some(&b"abc"[..])
    );
}

// --- Numbers ------------------------------------------------------------------

#[test]
fn int64_accepts_every_documented_text_form() {
    for (text, expected) in [("0", 0i64), ("123", 123), ("-123", -123), ("+123", 123)] {
        assert_eq!(
            read(text.as_bytes()).unwrap().as_i64(),
            Some(expected),
            "{text}"
        );
    }
    assert_eq!(
        read(b"-9223372036854775808").unwrap().as_i64(),
        Some(i64::MIN)
    );
}

#[test]
fn uint64_accepts_both_documented_text_forms() {
    // Suffixed.
    assert_eq!(read(b"123u").unwrap().as_u64(), Some(123));
    assert_eq!(
        read(b"18446744073709551615u").unwrap().as_u64(),
        Some(u64::MAX)
    );

    // Bare, which the page also lists as a uint64 text form. Anything above
    // `i64::MAX` can only be a uint64.
    assert_eq!(
        read(b"9223372036854775808").unwrap().as_u64(),
        Some(9_223_372_036_854_775_808)
    );
    assert_eq!(
        read(b"18446744073709551615").unwrap().as_u64(),
        Some(u64::MAX)
    );

    // Below that boundary a bare decimal is still an int64.
    assert_eq!(
        read(b"10000000000000").unwrap().as_i64(),
        Some(10_000_000_000_000)
    );

    // Past u64 it is neither.
    assert!(read(b"18446744073709551616").is_err());
}

#[test]
fn double_accepts_every_documented_text_form() {
    for (text, expected) in [
        ("0.0", 0.0),
        ("-1.0", -1.0),
        ("1e-9", 1e-9),
        ("1.5E+9", 1.5e9),
        ("32E1", 320.0),
    ] {
        assert_eq!(
            read(text.as_bytes()).unwrap().as_f64(),
            Some(expected),
            "{text}"
        );
    }
}

#[test]
fn booleans_and_entity_use_the_documented_spellings() {
    assert_eq!(read(b"%true").unwrap().as_bool(), Some(true));
    assert_eq!(read(b"%false").unwrap().as_bool(), Some(false));
    assert_eq!(read(b"#").unwrap().node, YsonNode::Entity);

    for (bytes, expected) in [
        (&[0x04u8][..], YsonNode::Boolean(false)),
        (&[0x05][..], YsonNode::Boolean(true)),
        (b"#", YsonNode::Entity),
    ] {
        assert_eq!(
            Reader::new(bytes, YsonFormat::Binary)
                .read_value()
                .unwrap()
                .node,
            expected
        );
    }
}

// --- Composite types and attributes -------------------------------------------

#[test]
fn the_pages_worked_examples_all_parse() {
    for example in [
        r#"[1; "hello"; {a=1; b=2}]"#,
        r#"{a = "hello"; "38 parrots" = [38]}"#,
        r#"<"44" = 44>44"#,
        r#"<id="aaad6921-b5704588-17990259-7b88bad3">#"#,
        "{ performance = 1 ; precision = 0.78 ; recall = 0.21 }",
        "{ cv-precision = [ 0.85 ; 0.24 ; 0.71 ; 0.70 ] }",
        "[ 1; 2; 3; 4; 5 ]",
        "foobar",
        r#""hello world""#,
        "42",
        "3.1415926",
        "{ home = { sandello = { mytable = <type = table> # ; \
         anothertable = <type = table> # } ; monster = { } } }",
        // Trailing separators are optional, both spellings valid.
        "<a=b;>c",
        "<a=b>c",
        "{a=b;}",
        "{a=b}",
    ] {
        assert!(read(example.as_bytes()).is_ok(), "did not parse: {example}");
    }
}

#[test]
fn attributes_attach_to_every_literal_kind() {
    for example in [
        "<k=1>42",
        "<k=1>#",
        "<k=1>[1;2]",
        "<k=1>{a=1}",
        "<k=1>str",
        "<a=<b=1>2>3",
    ] {
        let value = read(example.as_bytes()).unwrap();
        assert!(value.attributes.is_some(), "{example}");
    }
}

#[test]
fn a_list_fragment_is_semicolon_separated_with_an_optional_trailing_one() {
    let fragment = "{ key = a; value = 0 };\n{ key = b; value = 1 };\n{ key = c; value = 2 }";
    assert_eq!(
        Frames::new(fragment.as_bytes(), YsonFormat::Text).count(),
        3
    );
    assert_eq!(Frames::new(b"1;2;3", YsonFormat::Text).count(), 3);
    assert_eq!(Frames::new(b"1;2;3;", YsonFormat::Text).count(), 3);
}

#[test]
fn whitespace_is_ignored_between_any_two_tokens() {
    let spaced = "  <  a  =  1  >  {  b  =  [  1  ;  2  ]  }  ";
    let tight = "<a=1>{b=[1;2]}";
    assert_eq!(
        read(spaced.as_bytes()).unwrap(),
        read(tight.as_bytes()).unwrap()
    );
}

// --- Known deviations ---------------------------------------------------------

/// These are the places this crate and the page disagree. Each is a live test
/// so that the deviation is checked, not merely claimed; when one is fixed, the
/// test fails and says so.
mod known_deviations {
    use super::*;

    #[test]
    fn map_fragment_is_not_supported() {
        // The page names three data types: Node, ListFragment and MapFragment.
        // Only the first two have an entry point here.
        let map_fragment = "do = create; type = table; scheme = {}";
        assert!(Frames::new(map_fragment.as_bytes(), YsonFormat::Text).any(|f| f.is_err()));
    }

    #[test]
    fn an_empty_key_is_accepted_although_the_grammar_forbids_it() {
        // `<key-value-pair> = <string>, "=", <tree>;  % Key cannot be empty`
        assert!(read(br#"{""=1}"#).is_ok());
    }

    #[test]
    fn comments_are_accepted_although_the_grammar_has_none() {
        // An extension: leniency on input, so it cannot break reading valid YSON.
        assert_eq!(read(b"// c\n42").unwrap().as_i64(), Some(42));
        assert_eq!(read(b"/* c */ 42").unwrap().as_i64(), Some(42));
    }

    #[test]
    fn a_missing_separator_is_accepted() {
        // `[1 2]` is not in the grammar. `scan_value` matches the reader here on
        // purpose: a scanner stricter than the parser would frame records the
        // parser then rejects.
        assert!(read(b"[1 2]").is_ok());
    }
}
