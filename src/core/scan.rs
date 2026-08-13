//! Finding where a value ends, without decoding it.
//!
//! [`scan_value`] reports the byte length of the first complete value in a
//! buffer, or says that more bytes are needed. It walks the token stream
//! and builds nothing, so it never allocates -- which is what makes
//! consuming an input larger than memory possible.
//!
//! ```
//! use yson_rs::{Scan, YsonFormat, scan_value};
//!
//! let buffer = b"{a=1;b=2};{c=3}";
//! assert_eq!(scan_value(buffer, YsonFormat::Text).unwrap(), Scan::Complete(9));
//! assert_eq!(scan_value(b"{a=1;b=", YsonFormat::Text).unwrap(), Scan::Incomplete);
//! ```

use crate::core::DEFAULT_MAX_DEPTH;
use crate::core::error::YsonError;
use crate::core::format::YsonFormat;
use crate::core::reader::Reader;
use crate::core::token::TokenKind;

/// What [`scan_value`] found at the front of a buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scan {
    /// A whole value occupies the first `n` bytes of the input.
    ///
    /// In text mode this includes any insignificant bytes in front of the
    /// value, so the count is always a prefix length and never an offset the
    /// caller has to adjust.
    Complete(usize),
    /// The buffer holds the start of a value, or nothing but insignificant
    /// bytes. More input is needed before the answer can change.
    Incomplete,
}

/// Reports the byte length of the first complete value in `input`.
///
/// A truncated value is [`Scan::Incomplete`], not an error: on a stream, "the
/// buffer ends here" is a normal thing to find, and the caller's answer is to
/// read more. A value that is malformed rather than merely short *is* an error,
/// because no amount of further input can fix it.
///
/// Separators between top-level values are not consumed: a caller reading a
/// list fragment steps over its own `;`, or uses [`crate::Frames`].
///
/// # Examples
///
/// ```
/// use yson_rs::{Scan, YsonFormat, scan_value};
///
/// // The length is a prefix length, so it can be used to slice directly.
/// let buffer = b"<a=b>{x=10};next";
/// let Scan::Complete(len) = scan_value(buffer, YsonFormat::Text).unwrap() else {
///     panic!("the first value is whole");
/// };
/// assert_eq!(&buffer[..len], b"<a=b>{x=10}");
///
/// // Running out of bytes asks for more rather than failing.
/// assert_eq!(scan_value(b"[1;2", YsonFormat::Text).unwrap(), Scan::Incomplete);
/// ```
///
/// # Errors
///
/// Returns [`YsonError`] if the bytes are not the start of a valid value, or if
/// the value nests deeper than [`crate::core::DEFAULT_MAX_DEPTH`].
pub fn scan_value(input: &[u8], format: YsonFormat) -> Result<Scan, YsonError> {
    scan_value_with_max_depth(input, format, DEFAULT_MAX_DEPTH)
}

/// Reports the byte length of the first complete value, with the caller's
/// depth limit. Pass the same limit to the decoder that reads the frame.
///
/// # Errors
///
/// As [`scan_value`].
pub fn scan_value_with_max_depth(
    input: &[u8],
    format: YsonFormat,
    max_depth: usize,
) -> Result<Scan, YsonError> {
    let mut reader = Reader::new(input, format);
    match scan_one(&mut reader, 0, max_depth) {
        Ok(()) => Ok(Scan::Complete(reader.position())),
        // Both spellings of "the input stopped" mean the same thing: `Eof` at a
        // token boundary, `UnexpectedEof` part way through one.
        Err(YsonError::Eof | YsonError::UnexpectedEof(_)) => Ok(Scan::Incomplete),
        Err(other) => Err(other),
    }
}

fn scan_one(reader: &mut Reader<'_>, depth: usize, max: usize) -> Result<(), YsonError> {
    if depth > max {
        return Err(YsonError::Custom("Recursion limit exceeded".into()));
    }

    if reader.peek_byte()? == b'<' {
        reader.skip_token()?;
        scan_pairs(reader, b'>', depth + 1, max)?;
    }

    match reader.skip_token()? {
        TokenKind::Entity
        | TokenKind::Boolean
        | TokenKind::Int64
        | TokenKind::Uint64
        | TokenKind::Double
        | TokenKind::String => Ok(()),
        TokenKind::BeginList => scan_items(reader, depth + 1, max),
        TokenKind::BeginMap => scan_pairs(reader, b'}', depth + 1, max),
        t => Err(YsonError::Custom(format!("Unexpected token: {t:?}"))),
    }
}

fn scan_items(reader: &mut Reader<'_>, depth: usize, max: usize) -> Result<(), YsonError> {
    if depth > max {
        return Err(YsonError::Custom("Recursion limit exceeded".into()));
    }

    loop {
        match reader.peek_byte()? {
            b']' => {
                reader.skip_token()?;
                return Ok(());
            }
            b';' => {
                reader.skip_token()?;
            }
            _ => scan_one(reader, depth, max)?,
        }
    }
}

fn scan_pairs(reader: &mut Reader<'_>, end: u8, depth: usize, max: usize) -> Result<(), YsonError> {
    if depth > max {
        return Err(YsonError::Custom("Recursion limit exceeded".into()));
    }

    loop {
        let peeked = reader.peek_byte()?;
        if peeked == end {
            reader.skip_token()?;
            return Ok(());
        }
        if peeked == b';' {
            reader.skip_token()?;
            continue;
        }

        match reader.skip_token()? {
            TokenKind::String => {}
            t => return Err(YsonError::Custom(format!("Expected a key, got {t:?}"))),
        }
        match reader.skip_token()? {
            TokenKind::KeyValueSeparator => {}
            t => return Err(YsonError::Custom(format!("Expected '=', got {t:?}"))),
        }
        scan_one(reader, depth, max)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn len_of(input: &[u8], format: YsonFormat) -> usize {
        match scan_value(input, format).unwrap() {
            Scan::Complete(n) => n,
            Scan::Incomplete => panic!("expected a complete value for {input:?}"),
        }
    }

    #[test]
    fn a_whole_value_measures_itself() {
        for input in [
            &b"#"[..],
            b"42",
            b"%true",
            b"hello",
            b"\"a;b\"",
            b"[]",
            b"{}",
            b"[1;2;3]",
            b"{a=1;b=2}",
            b"<a=b>{x=10}",
            b"[[1;2];[3;4]]",
            b"<a=<b=1>2>[#]",
        ] {
            assert_eq!(
                len_of(input, YsonFormat::Text),
                input.len(),
                "for {input:?}"
            );
        }
    }

    #[test]
    fn the_length_is_the_first_value_only() {
        let buffer = b"{a=1};{b=2}";
        assert_eq!(len_of(buffer, YsonFormat::Text), 5);
        assert_eq!(&buffer[..5], b"{a=1}");

        let list = b"[1;2] [3]";
        assert_eq!(len_of(list, YsonFormat::Text), 5);
    }

    #[test]
    fn leading_whitespace_is_part_of_the_prefix() {
        // The count is a prefix length, so the caller can slice with it.
        let buffer = b"  \n 42 ";
        let len = len_of(buffer, YsonFormat::Text);
        assert_eq!(&buffer[..len], b"  \n 42");
    }

    #[test]
    fn a_truncated_value_asks_for_more() {
        for input in [
            &b""[..],
            b"[",
            b"[1;",
            b"[1;2",
            b"{",
            b"{a",
            b"{a=",
            b"{a=1",
            b"<",
            b"<a=b>",
            b"\"unterminated",
            b"   ",
        ] {
            assert_eq!(
                scan_value(input, YsonFormat::Text).unwrap(),
                Scan::Incomplete,
                "for {input:?}"
            );
        }
    }

    #[test]
    fn truncation_at_every_offset_is_incomplete_or_shorter() {
        let full = b"<a=b;c=[1;2]>{x=10;y=[{z=#}]}";
        for cut in 0..full.len() {
            let head = &full[..cut];
            match scan_value(head, YsonFormat::Text) {
                Ok(Scan::Incomplete) => {}
                Ok(Scan::Complete(n)) => panic!("prefix of {cut} bytes reported {n} complete"),
                Err(e) => panic!("prefix of {cut} bytes errored: {e}"),
            }
        }
        assert_eq!(len_of(full, YsonFormat::Text), full.len());
    }

    #[test]
    fn malformed_input_is_an_error_not_a_short_read() {
        // No amount of further input turns these into a value.
        for input in [&b"]"[..], b"=", b"{1=2}", b"/a", b"%maybe", b">"] {
            assert!(
                scan_value(input, YsonFormat::Text).is_err(),
                "accepted {input:?}"
            );
        }
    }

    #[test]
    fn scanning_accepts_exactly_what_the_reader_accepts() {
        // Framing is only useful if the two agree, so this pins them together
        // rather than pinning either to the grammar. `[1 2]` is in the list
        // because this crate's reader takes a missing `;` between items, and a
        // scanner that refused it would frame records the reader can read.
        for input in [
            &b"[1 2]"[..],
            b"{a=1 b=2}",
            b"[;;1;;]",
            b"<>#",
            b"[1;2]",
            b"]",
            b"{1=2}",
            b"%maybe",
        ] {
            let scanned = scan_value(input, YsonFormat::Text);
            let read = Reader::new(input, YsonFormat::Text).read_value();
            assert_eq!(
                scanned.is_ok(),
                read.is_ok(),
                "scan and reader disagree on {input:?}: {scanned:?} vs {read:?}"
            );
            if let Ok(Scan::Complete(n)) = scanned {
                assert_eq!(n, input.len(), "wrong length for {input:?}");
            }
        }
    }

    #[test]
    fn binary_values_measure_themselves() {
        // `<a=1>{b=[2]}` in binary YSON.
        let input: &[u8] = &[
            b'<', 0x01, 0x02, b'a', b'=', 0x02, 0x02, b'>', b'{', 0x01, 0x02, b'b', b'=', b'[',
            0x02, 0x04, b']', b'}',
        ];
        assert_eq!(len_of(input, YsonFormat::Binary), input.len());

        for cut in 0..input.len() {
            assert_eq!(
                scan_value(&input[..cut], YsonFormat::Binary).unwrap(),
                Scan::Incomplete,
                "binary prefix of {cut} bytes"
            );
        }
    }

    #[test]
    fn a_binary_string_longer_than_the_buffer_asks_for_more() {
        // A string header claiming 100 bytes with only three present.
        let input: &[u8] = &[0x01, 0xC8, 0x01, b'a', b'b', b'c'];
        assert_eq!(
            scan_value(input, YsonFormat::Binary).unwrap(),
            Scan::Incomplete
        );
    }

    #[test]
    fn a_caller_can_tighten_the_depth_limit() {
        // The scanner and the decoder must agree, so a framing loop that wants
        // a tighter bound passes the same one to both.
        let nested = b"[[[[1]]]]";
        assert!(matches!(
            scan_value_with_max_depth(nested, YsonFormat::Text, 16),
            Ok(Scan::Complete(_))
        ));
        assert!(scan_value_with_max_depth(nested, YsonFormat::Text, 2).is_err());
        assert!(
            Reader::new(nested, YsonFormat::Text)
                .read_value_with_max_depth(2)
                .is_err()
        );
    }

    #[test]
    fn the_depth_limit_is_enforced() {
        let deep = b"[".repeat(DEFAULT_MAX_DEPTH + 10);
        assert!(scan_value(&deep, YsonFormat::Text).is_err());
    }

    #[test]
    fn framing_a_stream_walks_it_to_the_end() {
        let stream = b"{a=1};{b=2};{c=3}";
        let mut rest = &stream[..];
        let mut found = 0;

        while !rest.is_empty() {
            if rest[0] == b';' {
                rest = &rest[1..];
                continue;
            }
            let Scan::Complete(len) = scan_value(rest, YsonFormat::Text).unwrap() else {
                panic!("the stream is whole");
            };
            assert!(
                Reader::new(&rest[..len], YsonFormat::Text)
                    .read_value()
                    .is_ok()
            );
            rest = &rest[len..];
            found += 1;
        }

        assert_eq!(found, 3);
    }
}
