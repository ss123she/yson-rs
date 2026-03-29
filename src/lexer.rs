use std::borrow::Cow;

use crate::{error::YsonError, node::Token};

pub struct YsonIterator<'a> {
    input: &'a [u8],
    pos: usize,
    pub(crate) is_binary: bool,
    peeked: Option<Result<Token<'a>, YsonError>>,
}

impl<'a> YsonIterator<'a> {
    pub fn new(input: &'a [u8], is_binary: bool) -> Self {
        Self {
            input,
            pos: 0,
            is_binary,
            peeked: None,
        }
    }

    pub fn peek(&mut self) -> Result<&Token<'a>, YsonError> {
        if self.peeked.is_none() {
            self.peeked = Some(self.next_token_impl());
        }

        self.peeked
            .as_ref()
            .unwrap()
            .as_ref()
            .map_err(|e| e.clone())
    }

    pub fn next_token(&mut self) -> Result<Token<'a>, YsonError> {
        if let Some(token) = self.peeked.take() {
            return token;
        }
        self.next_token_impl()
    }

    fn next_token_impl(&mut self) -> Result<Token<'a>, YsonError> {
        if !self.is_binary {
            self.skip_ignored();
        }

        if self.pos >= self.input.len() {
            return Err(YsonError::Eof);
        }

        if self.is_binary {
            self.parse_binary_token()
        } else {
            self.parse_text_token()
        }
    }

    fn parse_binary_token(&mut self) -> Result<Token<'a>, YsonError> {
        let byte = self.input[self.pos];
        self.pos += 1;

        match byte {
            0x01 => {
                // String 0x01 + length + data(<length> bytes)

                let (len, read) = crate::varint::read_varint(&self.input[self.pos..])?;
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

                let (val, read) = crate::varint::read_varint(&self.input[self.pos..])?;
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

                let (val, read) = crate::varint::read_uvarint(&self.input[self.pos..])?;
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

            b'%' => self.parse_text_boolean(),

            _ if byte.is_ascii_alphabetic() || byte == b'_' => self.parse_text_unquoted_string(),

            _ => Err(YsonError::InvalidMarker(byte, self.pos)),
        }
    }

    pub fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() && self.input[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn skip_ignored(&mut self) {
        while self.pos < self.input.len() {
            let byte = self.input[self.pos];

            if byte.is_ascii_whitespace() {
                self.pos += 1;
                continue;
            }

            if byte == b'/' && self.pos + 1 < self.input.len() {
                let next_byte = self.input[self.pos + 1];

                if next_byte == b'/' {
                    self.pos += 2;
                    while self.pos < self.input.len() && self.input[self.pos] != b'\n' {
                        self.pos += 1;
                    }
                } else if next_byte == b'*' {
                    self.pos += 2;
                    while self.pos + 1 < self.input.len() {
                        if self.input[self.pos] == b'*' && self.input[self.pos + 1] == b'/' {
                            self.pos += 2;
                            break;
                        }
                        self.pos += 1;
                    }
                }
                continue;
            }
            break;
        }
    }

    fn parse_text_quoted_string(&mut self) -> Result<Token<'a>, YsonError> {
        self.pos += 1;
        let start = self.pos;
        let mut has_escapes = false;

        let mut current = self.pos;
        while current < self.input.len() {
            match self.input[current] {
                b'"' => break,
                b'\\' => {
                    has_escapes = true;
                    current += 2;
                }
                _ => current += 1,
            }
        }

        if current >= self.input.len() {
            return Err(YsonError::UnexpectedEof(current));
        }

        if !has_escapes {
            let slice = &self.input[start..current];
            self.pos = current + 1;
            return Ok(Token::String(Cow::Borrowed(slice)));
        }

        let mut buf = Vec::with_capacity(current - start);
        while self.pos < self.input.len() {
            let b = self.input[self.pos];
            if b == b'"' {
                self.pos += 1;
                return Ok(Token::String(Cow::Owned(buf)));
            } else if b == b'\\' {
                self.pos += 1;
                if self.pos >= self.input.len() {
                    return Err(YsonError::UnexpectedEof(self.pos));
                }
                match self.input[self.pos] {
                    b'"' => buf.push(b'"'),
                    b'\\' => buf.push(b'\\'),
                    b'n' => buf.push(b'\n'),
                    b'r' => buf.push(b'\r'),
                    b't' => buf.push(b'\t'),
                    b'x' => {
                        if self.pos + 2 >= self.input.len() {
                            return Err(YsonError::UnexpectedEof(self.pos));
                        }
                        let hex = std::str::from_utf8(&self.input[self.pos + 1..self.pos + 3])
                            .map_err(|_| YsonError::Custom("Invalid hex escape".into()))?;
                        let byte = u8::from_str_radix(hex, 16)
                            .map_err(|_| YsonError::Custom("Invalid hex escape".into()))?;
                        buf.push(byte);
                        self.pos += 2;
                    }
                    c => buf.push(c),
                }
            } else {
                buf.push(b);
            }
            self.pos += 1;
        }

        Err(YsonError::UnexpectedEof(self.pos))
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
                .map_err(|_| YsonError::Custom(format!("Invalid uint64: {}", s)))?;
            Ok(Token::Uint64(val))
        } else if has_dot_or_exp {
            let val = s
                .parse::<f64>()
                .map_err(|_| YsonError::Custom(format!("Invalid double: {}", s)))?;
            Ok(Token::Double(val))
        } else {
            let val = s
                .parse::<i64>()
                .map_err(|_| YsonError::Custom(format!("Invalid int64: {}", s)))?;
            Ok(Token::Int64(val))
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

    fn parse_text_boolean(&mut self) -> Result<Token<'a>, YsonError> {
        self.pos += 1;
        let remaining = &self.input[self.pos..];

        if remaining.starts_with(b"true") {
            self.pos += 4;
            Ok(Token::Boolean(true))
        } else if remaining.starts_with(b"false") {
            self.pos += 5;
            Ok(Token::Boolean(false))
        } else {
            Err(YsonError::Custom(
                "Invalid boolean: expected 'true' or 'false' after '%'".into(),
            ))
        }
    }
}
