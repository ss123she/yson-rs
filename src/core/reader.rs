use std::borrow::Cow;

use crate::core::DEFAULT_MAX_DEPTH;
use crate::core::error::YsonError;
use crate::core::format::YsonFormat;
use crate::core::node::{YsonMap, YsonNode, YsonValue};
use crate::core::token::{Token, TokenKind};
use crate::core::varint;

/// A token-at-a-time reader over a YSON byte slice, in either format.
///
/// Tokens borrow the input where they can, which is what makes zero-copy
/// decoding possible. [`Reader::read_value`] builds a whole [`YsonValue`] on
/// top of the same token stream, without serde.
///
/// # Examples
///
/// ```
/// use yson_rs::core::{Reader, Token, YsonFormat};
///
/// let mut reader = Reader::new(b"[1;2]", YsonFormat::Text);
/// assert_eq!(reader.next_token().unwrap(), Token::BeginList);
/// assert_eq!(reader.next_token().unwrap(), Token::Int64(1));
/// ```
pub struct Reader<'a> {
    input: &'a [u8],
    pos: usize,
    format: YsonFormat,
}

impl<'a> Reader<'a> {
    /// Creates a reader over `input`, interpreted in `format`.
    #[must_use]
    pub fn new(input: &'a [u8], format: YsonFormat) -> Self {
        Self {
            input,
            pos: 0,
            format,
        }
    }

    /// The format this reader was built with.
    #[must_use]
    pub const fn format(&self) -> YsonFormat {
        self.format
    }

    /// The reader's current byte offset into the input.
    #[must_use]
    pub const fn position(&self) -> usize {
        self.pos
    }

    /// Returns the next meaningful byte without consuming it.
    ///
    /// In text mode this first skips whitespace and comments. The byte is the
    /// raw marker, so the structural characters (`<`, `[`, `{`, `#`, `;`, `=`)
    /// compare equal in both formats.
    ///
    /// # Errors
    ///
    /// Returns [`YsonError::Eof`] when the input is exhausted.
    pub fn peek_byte(&mut self) -> Result<u8, YsonError> {
        if !self.format.is_binary() {
            self.skip_ignored();
        }
        if self.pos >= self.input.len() {
            return Err(YsonError::Eof);
        }
        Ok(self.input[self.pos])
    }

    /// Advances past the next token, reporting only its shape.
    ///
    /// Never builds a value, so it never allocates: a text string carrying
    /// escapes is stepped over rather than decoded.
    ///
    /// # Examples
    ///
    /// ```
    /// use yson_rs::{Reader, TokenKind, YsonFormat};
    ///
    /// // The escape is stepped over, not decoded.
    /// let mut reader = Reader::new(br#""a\nb";1"#, YsonFormat::Text);
    /// assert_eq!(reader.skip_token().unwrap(), TokenKind::String);
    /// assert_eq!(reader.skip_token().unwrap(), TokenKind::ItemSeparator);
    /// ```
    ///
    /// # Errors
    ///
    /// As [`Reader::next_token`].
    pub fn skip_token(&mut self) -> Result<TokenKind, YsonError> {
        if !self.format.is_binary() {
            self.skip_ignored();
            if self.pos < self.input.len() && self.input[self.pos] == b'"' {
                self.text_string_bounds()?;
                return Ok(TokenKind::String);
            }
        }
        Ok(self.next_token()?.kind())
    }

    /// Consumes and returns the next token.
    ///
    /// # Errors
    ///
    /// Returns [`YsonError::Eof`] at the end of the input, or a parse error if
    /// the next bytes are not a valid token.
    pub fn next_token(&mut self) -> Result<Token<'a>, YsonError> {
        if !self.format.is_binary() {
            self.skip_ignored();
        }
        if self.pos >= self.input.len() {
            return Err(YsonError::Eof);
        }
        if self.format.is_binary() {
            self.parse_binary_token()
        } else {
            self.parse_text_token()
        }
    }

    /// Reads one complete [`YsonValue`], attributes included.
    ///
    /// Map keys, attribute names and string values are kept as bytes, so a
    /// value whose names are not UTF-8 reads like any other.
    ///
    /// # Examples
    ///
    /// ```
    /// use yson_rs::core::{Reader, YsonFormat};
    ///
    /// let mut reader = Reader::new(b"<lang=rust>{n=1}", YsonFormat::Text);
    /// let value = reader.read_value().unwrap();
    ///
    /// assert_eq!(value.attr("lang").unwrap().as_str(), Some("rust"));
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`YsonError`] if the input is malformed, ends early, or nests
    /// deeper than [`crate::core::DEFAULT_MAX_DEPTH`].
    pub fn read_value(&mut self) -> Result<YsonValue<'a>, YsonError> {
        self.read_value_with_max_depth(DEFAULT_MAX_DEPTH)
    }

    /// Reads one complete [`YsonValue`], refusing input nested deeper than `max_depth`.
    ///
    /// Nesting is the one dimension where a small input can cost unbounded
    /// stack, so a caller reading untrusted input may want a tighter bound.
    ///
    /// # Errors
    ///
    /// As [`Reader::read_value`], with the caller's depth limit.
    pub fn read_value_with_max_depth(
        &mut self,
        max_depth: usize,
    ) -> Result<YsonValue<'a>, YsonError> {
        self.read_value_at(0, max_depth)
    }

    fn read_value_at(&mut self, depth: usize, max: usize) -> Result<YsonValue<'a>, YsonError> {
        if depth > max {
            return Err(YsonError::Custom("Recursion limit exceeded".into()));
        }

        let attributes = if self.peek_byte()? == b'<' {
            self.next_token()?;
            Some(self.read_pairs(b'>', depth + 1, max)?)
        } else {
            None
        };

        let node = self.read_node(depth, max)?;
        Ok(YsonValue { attributes, node })
    }

    fn read_node(&mut self, depth: usize, max: usize) -> Result<YsonNode<'a>, YsonError> {
        match self.next_token()? {
            Token::Entity => Ok(YsonNode::Entity),
            Token::Boolean(v) => Ok(YsonNode::Boolean(v)),
            Token::Int64(v) => Ok(YsonNode::Int64(v)),
            Token::Uint64(v) => Ok(YsonNode::Uint64(v)),
            Token::Double(v) => Ok(YsonNode::Double(v)),
            Token::String(s) => Ok(YsonNode::String(s)),
            Token::BeginList => {
                if depth + 1 > max {
                    return Err(YsonError::Custom("Recursion limit exceeded".into()));
                }
                let mut items = Vec::new();
                loop {
                    let peeked = self.peek_byte()?;
                    if peeked == b']' {
                        self.next_token()?;
                        break;
                    }
                    if peeked == b';' {
                        self.next_token()?;
                        continue;
                    }
                    items.push(self.read_value_at(depth + 1, max)?);
                }
                Ok(YsonNode::List(items))
            }
            Token::BeginMap => Ok(YsonNode::Map(self.read_pairs(b'}', depth + 1, max)?)),
            t => Err(YsonError::Custom(format!("Unexpected token: {t:?}"))),
        }
    }

    fn read_pairs(&mut self, end: u8, depth: usize, max: usize) -> Result<YsonMap<'a>, YsonError> {
        if depth > max {
            return Err(YsonError::Custom("Recursion limit exceeded".into()));
        }

        let mut entries = YsonMap::new();
        loop {
            let peeked = self.peek_byte()?;
            if peeked == end {
                self.next_token()?;
                break;
            }
            if peeked == b';' {
                self.next_token()?;
                continue;
            }

            let key = match self.next_token()? {
                Token::String(s) => s,
                t => return Err(YsonError::Custom(format!("Expected a key, got {t:?}"))),
            };

            match self.next_token()? {
                Token::KeyValueSeparator => {}
                t => return Err(YsonError::Custom(format!("Expected '=', got {t:?}"))),
            }

            entries.insert(key, self.read_value_at(depth, max)?);
        }
        Ok(entries)
    }

    /// Shifts a varint error's offset from inside the varint to the whole input.
    fn absolute(&self, error: YsonError) -> YsonError {
        match error {
            YsonError::UnexpectedEof(relative) => YsonError::UnexpectedEof(self.pos + relative),
            other => other,
        }
    }

    fn parse_binary_token(&mut self) -> Result<Token<'a>, YsonError> {
        let byte = self.input[self.pos];
        self.pos += 1;

        match byte {
            0x01 => {
                // String 0x01 + length + data(<length> bytes)

                let (len, read) =
                    varint::read_varint(&self.input[self.pos..]).map_err(|e| self.absolute(e))?;
                self.pos += read;
                if len < 0 {
                    return Err(YsonError::Custom("String length cannot be negative".into()));
                }
                let len = len as usize;
                let s = self
                    .input
                    .get(self.pos..self.pos + len)
                    .ok_or(YsonError::UnexpectedEof(self.pos))?;
                self.pos += len;
                Ok(Token::String(Cow::Borrowed(s)))
            }

            0x02 => {
                // Int64 0x02 + value

                let (val, read) =
                    varint::read_varint(&self.input[self.pos..]).map_err(|e| self.absolute(e))?;
                self.pos += read;
                Ok(Token::Int64(val))
            }

            0x03 => {
                // Double 0x03 + double

                let bytes = self
                    .input
                    .get(self.pos..self.pos + 8)
                    .ok_or(YsonError::UnexpectedEof(self.pos))?;
                let val = f64::from_le_bytes(bytes.try_into().unwrap());
                self.pos += 8;
                Ok(Token::Double(val))
            }

            // Boolean 0x04 => False | 0x05 => True
            0x04 => Ok(Token::Boolean(false)),
            0x05 => Ok(Token::Boolean(true)),

            0x06 => {
                // UInt64 0x06 + value

                let (val, read) =
                    varint::read_uvarint(&self.input[self.pos..]).map_err(|e| self.absolute(e))?;
                self.pos += read;
                Ok(Token::Uint64(val))
            }

            b'#' => Ok(Token::Entity),            // 0x23
            b'<' => Ok(Token::BeginAttributes),   // 0x3C
            b'>' => Ok(Token::EndAttributes),     // 0x3E
            b'[' => Ok(Token::BeginList),         // 0x5B
            b']' => Ok(Token::EndList),           // 0x5D
            b'{' => Ok(Token::BeginMap),          // 0x7B
            b'}' => Ok(Token::EndMap),            // 0x7D
            b'=' => Ok(Token::KeyValueSeparator), // 0x3D
            b';' => Ok(Token::ItemSeparator),     // 0x3B

            _ => Err(YsonError::InvalidMarker(byte, self.pos - 1)),
        }
    }

    fn parse_text_token(&mut self) -> Result<Token<'a>, YsonError> {
        let byte = self.input[self.pos];

        match byte {
            b'[' => {
                self.pos += 1;
                Ok(Token::BeginList)
            }
            b']' => {
                self.pos += 1;
                Ok(Token::EndList)
            }
            b'{' => {
                self.pos += 1;
                Ok(Token::BeginMap)
            }
            b'}' => {
                self.pos += 1;
                Ok(Token::EndMap)
            }
            b'<' => {
                self.pos += 1;
                Ok(Token::BeginAttributes)
            }
            b'>' => {
                self.pos += 1;
                Ok(Token::EndAttributes)
            }
            b'=' => {
                self.pos += 1;
                Ok(Token::KeyValueSeparator)
            }
            b';' => {
                self.pos += 1;
                Ok(Token::ItemSeparator)
            }
            b'#' => {
                self.pos += 1;
                Ok(Token::Entity)
            }

            b'"' => self.parse_text_quoted_string(),

            b'0'..=b'9' | b'-' | b'+' => self.parse_text_number(),

            b'%' => self.parse_text_special_value(),

            _ if byte.is_ascii_alphabetic() || byte == b'_' => self.parse_text_unquoted_string(),

            _ => Err(YsonError::InvalidMarker(byte, self.pos)),
        }
    }

    /// Skips whitespace and comments in text mode.
    ///
    /// A `/` that opens no comment is not ignorable: it breaks out and is left
    /// for the tokenizer to reject. Every branch must advance `pos`.
    fn skip_ignored(&mut self) {
        while self.pos < self.input.len() {
            let byte = self.input[self.pos];

            if byte.is_ascii_whitespace() {
                self.pos += 1;
                continue;
            }

            if byte == b'/' && self.pos + 1 < self.input.len() {
                match self.input[self.pos + 1] {
                    b'/' => {
                        self.pos += 2;
                        while self.pos < self.input.len() && self.input[self.pos] != b'\n' {
                            self.pos += 1;
                        }
                        continue;
                    }
                    b'*' => {
                        self.pos += 2;
                        let mut terminated = false;
                        while self.pos + 1 < self.input.len() {
                            if self.input[self.pos] == b'*' && self.input[self.pos + 1] == b'/' {
                                self.pos += 2;
                                terminated = true;
                                break;
                            }
                            self.pos += 1;
                        }
                        // An unterminated block comment runs to end of input.
                        if !terminated {
                            self.pos = self.input.len();
                        }
                        continue;
                    }
                    _ => break,
                }
            }
            break;
        }
    }

    /// Finds the end of a quoted string and advances past it, decoding nothing.
    ///
    /// Returns the raw bytes between the quotes and whether they carry escapes.
    /// Separating this from decoding is what lets [`Reader::skip_token`] walk a
    /// document without allocating.
    fn text_string_bounds(&mut self) -> Result<(&'a [u8], bool), YsonError> {
        self.pos += 1; // the opening quote
        let start = self.pos;
        let mut has_escapes = false;
        let mut cursor = start;

        while cursor < self.input.len() {
            match self.input[cursor] {
                b'"' => {
                    self.pos = cursor + 1;
                    return Ok((&self.input[start..cursor], has_escapes));
                }
                b'\\' => {
                    has_escapes = true;
                    cursor += 2;
                }
                _ => cursor += 1,
            }
        }

        Err(YsonError::UnexpectedEof(cursor))
    }

    fn parse_text_quoted_string(&mut self) -> Result<Token<'a>, YsonError> {
        let (raw, has_escapes) = self.text_string_bounds()?;
        if has_escapes {
            Ok(Token::String(Cow::Owned(decode_escapes(raw)?)))
        } else {
            Ok(Token::String(Cow::Borrowed(raw)))
        }
    }

    fn parse_text_number(&mut self) -> Result<Token<'a>, YsonError> {
        let start = self.pos;
        let mut has_dot_or_exp = false;
        let mut is_unsigned = false;

        while self.pos < self.input.len() {
            let b = self.input[self.pos];
            match b {
                b'0'..=b'9' | b'-' | b'+' => {}
                b'.' | b'e' | b'E' => {
                    has_dot_or_exp = true;
                }
                b'u' => {
                    is_unsigned = true;
                    self.pos += 1;
                    break;
                }
                _ => break,
            }
            self.pos += 1;
        }

        let slice = &self.input[start..self.pos];

        let s = std::str::from_utf8(slice)
            .map_err(|_| YsonError::Custom("Invalid UTF-8 in number".into()))?;

        if is_unsigned {
            let val = s
                .trim_end_matches('u')
                .parse::<u64>()
                .map_err(|_| YsonError::Custom(format!("Invalid uint64: {s}")))?;
            Ok(Token::Uint64(val))
        } else if has_dot_or_exp {
            let val = s
                .parse::<f64>()
                .map_err(|_| YsonError::Custom(format!("Invalid double: {s}")))?;
            Ok(Token::Double(val))
        } else {
            // A bare decimal is a text form of *both* int64 and uint64, so one
            // that does not fit the first can only be the second. Only trying
            // `i64` rejected every value above `i64::MAX` written without the
            // `u` suffix.
            match s.parse::<i64>() {
                Ok(val) => Ok(Token::Int64(val)),
                Err(_) => s
                    .parse::<u64>()
                    .map(Token::Uint64)
                    .map_err(|_| YsonError::Custom(format!("Invalid int64: {s}"))),
            }
        }
    }

    fn parse_text_unquoted_string(&mut self) -> Result<Token<'a>, YsonError> {
        let start = self.pos;
        while self.pos < self.input.len() {
            let b = self.input[self.pos];
            if b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.' {
                self.pos += 1;
            } else {
                break;
            }
        }

        let slice = &self.input[start..self.pos];
        if slice.is_empty() {
            return Err(YsonError::Custom("Empty unquoted string".into()));
        }

        Ok(Token::String(Cow::Borrowed(slice)))
    }

    fn parse_text_special_value(&mut self) -> Result<Token<'a>, YsonError> {
        const SPECIALS: [&[u8]; 5] = [b"false", b"true", b"-inf", b"nan", b"inf"];

        self.pos += 1;
        let remaining = &self.input[self.pos..];

        for word in SPECIALS {
            if remaining.starts_with(word) {
                self.pos += word.len();
                return Ok(match word {
                    b"true" => Token::Boolean(true),
                    b"false" => Token::Boolean(false),
                    b"nan" => Token::Double(f64::NAN),
                    b"-inf" => Token::Double(f64::NEG_INFINITY),
                    _ => Token::Double(f64::INFINITY),
                });
            }
        }

        // `%tr` is a prefix of `%true`, so the input stopped part way through
        // the token rather than being wrong. Framing needs that distinction.
        if SPECIALS.iter().any(|word| word.starts_with(remaining)) {
            return Err(YsonError::UnexpectedEof(self.pos));
        }

        Err(YsonError::Custom(
            "Invalid special value: expected 'true', 'false', 'nan', 'inf' or '-inf' after '%'"
                .into(),
        ))
    }
}

/// Resolves the backslash escapes in the body of a quoted text string.
///
/// This is the one place reading a document has to allocate: the decoded bytes
/// of `"a\nb"` exist nowhere in the input, so they cannot be borrowed from it.
fn decode_escapes(raw: &[u8]) -> Result<Vec<u8>, YsonError> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;

    while i < raw.len() {
        if raw[i] != b'\\' {
            out.push(raw[i]);
            i += 1;
            continue;
        }

        i += 1;
        if i >= raw.len() {
            return Err(YsonError::UnexpectedEof(i));
        }

        match raw[i] {
            // The whole C set. Leaving `\a \b \f \v` out of it did not fail --
            // they fell through to the catch-all below and decoded to the
            // letter, so a backspace silently became a `b`.
            b'a' => out.push(0x07),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0C),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(0x0B),
            b'\\' => out.push(b'\\'),
            b'"' => out.push(b'"'),
            b'\'' => out.push(b'\''),
            b'?' => out.push(b'?'),
            b'x' => {
                if i + 2 >= raw.len() {
                    return Err(YsonError::UnexpectedEof(i));
                }
                let hex = std::str::from_utf8(&raw[i + 1..i + 3])
                    .map_err(|_| YsonError::Custom("Invalid hex escape".into()))?;
                out.push(
                    u8::from_str_radix(hex, 16)
                        .map_err(|_| YsonError::Custom("Invalid hex escape".into()))?,
                );
                i += 2;
            }
            b'0'..=b'7' => {
                let mut val = raw[i] - b'0';
                if i + 1 < raw.len() && raw[i + 1].is_ascii_digit() && raw[i + 1] < b'8' {
                    val = val * 8 + (raw[i + 1] - b'0');
                    i += 1;
                    if i + 1 < raw.len() && raw[i + 1].is_ascii_digit() && raw[i + 1] < b'8' {
                        let wider = u16::from(val) * 8 + u16::from(raw[i + 1] - b'0');
                        if wider <= 255 {
                            val = wider as u8;
                            i += 1;
                        }
                    }
                }
                out.push(val);
            }
            other => out.push(other),
        }
        i += 1;
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Writer;
    use std::collections::BTreeMap;

    fn round_trip(input: &[u8], format: YsonFormat) -> Vec<u8> {
        let value = Reader::new(input, format).read_value().unwrap();
        let mut out = Vec::new();
        Writer::new(&mut out, format).write_value(&value).unwrap();
        out
    }

    #[test]
    fn text_round_trip() {
        assert_eq!(
            round_trip(b"<a=1>{b=2;c=[1;2]}", YsonFormat::Text),
            b"<a=1>{b=2;c=[1;2]}"
        );
        assert_eq!(round_trip(b"#", YsonFormat::Text), b"#");
        assert_eq!(round_trip(b"[]", YsonFormat::Text), b"[]");
        assert_eq!(round_trip(b"{}", YsonFormat::Text), b"{}");
    }

    #[test]
    fn non_utf8_keys_and_attribute_names_survive() {
        let mut input = Vec::new();
        let mut w = Writer::new(&mut input, YsonFormat::Binary);
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

        let value = Reader::new(&input, YsonFormat::Binary)
            .read_value()
            .unwrap();

        assert!(value.attr_bytes(b"\xff\xfe").is_some());
        let YsonNode::Map(map) = &value.node else {
            panic!("expected a map");
        };
        assert_eq!(
            map[b"\x80key".as_slice()].as_bytes(),
            Some(&b"\xc3\x28"[..])
        );
    }

    #[test]
    fn depth_limit_is_enforced() {
        let deep = b"[".repeat(600);
        assert!(Reader::new(&deep, YsonFormat::Text).read_value().is_err());
    }

    #[test]
    fn stray_slash_is_refused_not_looped() {
        for input in [
            &b"/a"[..],
            b"/",
            b"/ ",
            b"{a=/}",
            b"[/]",
            b"//comment\n/x",
            b"/*done*/-",
        ] {
            let result = Reader::new(input, YsonFormat::Text).read_value();
            assert!(result.is_err(), "expected an error for {input:?}");
        }
    }

    #[test]
    fn unterminated_block_comment_reaches_eof() {
        let mut reader = Reader::new(b"1;/*never closed", YsonFormat::Text);
        assert_eq!(reader.read_value().unwrap().node, YsonNode::Int64(1));
        assert_eq!(reader.next_token(), Ok(Token::ItemSeparator));
        assert_eq!(reader.next_token(), Err(YsonError::Eof));
    }

    #[test]
    fn comments_still_work() {
        let cases: [(&[u8], YsonNode); 4] = [
            (b"// leading\n42", YsonNode::Int64(42)),
            (b"/* leading */ 42", YsonNode::Int64(42)),
            (b"42 // trailing", YsonNode::Int64(42)),
            (
                b"{a=/*inline*/1}",
                YsonNode::Map(BTreeMap::from([(
                    Cow::Borrowed(&b"a"[..]),
                    YsonValue::new(YsonNode::Int64(1)),
                )])),
            ),
        ];
        for (input, expected) in cases {
            let value = Reader::new(input, YsonFormat::Text).read_value().unwrap();
            assert_eq!(value.node, expected, "for {input:?}");
        }
    }

    #[test]
    fn a_slash_inside_a_string_is_untouched() {
        let value = Reader::new(b"\"a/b\"", YsonFormat::Text)
            .read_value()
            .unwrap();
        assert_eq!(value.as_bytes(), Some(&b"a/b"[..]));
    }
}
