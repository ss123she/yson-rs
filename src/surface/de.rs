use crate::core::YsonFormat;
use crate::core::error::YsonError;
use crate::core::reader::Reader;
use crate::core::token::Token;
use crate::surface::access::{
    AttributesWrapperAccess, CommaSeparated, EmptyMapAccess, EnumAccess, FlatStructAccess,
};
use serde::de::{self, Visitor};
use std::borrow::Cow;

/// A structure for deserializing YSON data into Rust types.
pub struct Deserializer<'de> {
    pub(crate) reader: Reader<'de>,
    pub(crate) is_reading_attributes: bool,
    depth: usize,
    max_depth: usize,
}

impl<'de> Deserializer<'de> {
    /// Creates a deserializer over `input`, read in `format`.
    ///
    /// # Examples
    ///
    /// ```
    /// use yson_rs::{Deserializer, YsonFormat};
    /// use serde::Deserialize;
    ///
    /// let mut de = Deserializer::new(b"42", YsonFormat::Text);
    /// assert_eq!(i64::deserialize(&mut de).unwrap(), 42);
    /// ```
    #[must_use]
    pub fn new(input: &'de [u8], format: YsonFormat) -> Self {
        Deserializer {
            reader: Reader::new(input, format),
            is_reading_attributes: false,
            depth: 0,
            max_depth: 128,
        }
    }

    /// Checks that the whole input has been read.
    ///
    /// Trailing whitespace and comments are allowed in text mode; in binary
    /// mode every remaining byte is trailing data.
    ///
    /// # Errors
    ///
    /// Returns [`YsonError`] naming the offset of the first trailing byte.
    pub fn end(&mut self) -> Result<(), YsonError> {
        match self.reader.peek_byte() {
            Err(YsonError::Eof) => Ok(()),
            Err(e) => Err(e),
            Ok(_) => Err(YsonError::Custom(format!(
                "Trailing data after the value, at offset {}",
                self.reader.position()
            ))),
        }
    }

    pub(crate) fn enter_recursion(&mut self) -> Result<(), YsonError> {
        self.depth += 1;
        if self.depth > self.max_depth {
            return Err(YsonError::Custom("Recursion limit exceeded".into()));
        }
        Ok(())
    }

    pub(crate) fn leave_recursion(&mut self) {
        self.depth -= 1;
    }

    /// Consumes the terminator of a container this deserializer opened.
    ///
    /// A visitor is not trusted to have read it: `Vec` asks for one element
    /// past the end and would, but a tuple, tuple struct or array asks
    /// exactly its length of times and stops. Whoever writes the opening
    /// token closes it, which also makes a container longer than the visitor
    /// read an error rather than a silent truncation.
    fn close_container(&mut self, end_byte: u8) -> Result<(), YsonError> {
        loop {
            let peeked = self.reader.peek_byte()?;
            if peeked == end_byte {
                self.reader.next_token()?;
                return Ok(());
            }
            if peeked == b';' {
                self.reader.next_token()?;
                continue;
            }
            return Err(YsonError::Custom(format!(
                "Expected '{}' to close the container, found more items at offset {}",
                end_byte as char,
                self.reader.position()
            )));
        }
    }

    /// Reads the elements of an already-opened list and consumes its `]`.
    fn visit_seq_container<V>(&mut self, visitor: V) -> Result<V::Value, YsonError>
    where
        V: Visitor<'de>,
    {
        let value = visitor.visit_seq(CommaSeparated::new(&mut *self, b']')?)?;
        self.close_container(b']')?;
        Ok(value)
    }

    /// Reads the entries of an already-opened map or attribute list and
    /// consumes its `}` or `>`.
    fn visit_map_container<V>(&mut self, end_byte: u8, visitor: V) -> Result<V::Value, YsonError>
    where
        V: Visitor<'de>,
    {
        let value = visitor.visit_map(CommaSeparated::new(&mut *self, end_byte)?)?;
        self.close_container(end_byte)?;
        Ok(value)
    }

    fn skip_attributes(&mut self) -> Result<(), YsonError> {
        if self.reader.peek_byte()? == b'<' {
            self.enter_recursion()?;
            self.reader.next_token()?;
            let mut attr_depth = 1;
            while attr_depth > 0 {
                match self.reader.next_token()? {
                    Token::BeginAttributes => attr_depth += 1,
                    Token::EndAttributes => attr_depth -= 1,
                    _ => {}
                }
                if attr_depth > self.max_depth {
                    return Err(YsonError::Custom("Attributes nesting too deep".into()));
                }
            }
            self.leave_recursion();
        }
        Ok(())
    }
}

macro_rules! delegate_skip_attributes {
    ( $($method:ident),* $(,)? ) => {
        $(
            fn $method<V>(self, visitor: V) -> Result<V::Value, Self::Error>
            where
                V: Visitor<'de>,
            {
                if !self.is_reading_attributes {
                    self.skip_attributes()?;
                }
                self.deserialize_any(visitor)
            }
        )*
    };
}

impl<'de> de::Deserializer<'de> for &mut Deserializer<'de> {
    type Error = YsonError;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let was_reading_attributes = self.is_reading_attributes;
        self.is_reading_attributes = false;

        if was_reading_attributes {
            if self.reader.peek_byte()? != b'<' {
                return visitor.visit_map(EmptyMapAccess);
            }
            self.reader.next_token()?;
            return self.visit_map_container(b'>', visitor);
        }

        if self.reader.peek_byte()? == b'<' {
            return visitor.visit_map(FlatStructAccess::new(self)?);
        }

        match self.reader.next_token()? {
            Token::Entity => visitor.visit_unit(),
            Token::Boolean(b) => visitor.visit_bool(b),
            Token::Int64(i) => visitor.visit_i64(i),
            Token::Uint64(u) => visitor.visit_u64(u),
            Token::Double(d) => visitor.visit_f64(d),
            Token::String(s) => match s {
                Cow::Borrowed(b) => {
                    if let Ok(utf8) = std::str::from_utf8(b) {
                        visitor.visit_borrowed_str(utf8)
                    } else {
                        visitor.visit_borrowed_bytes(b)
                    }
                }
                Cow::Owned(vec) => match String::from_utf8(vec) {
                    Ok(utf8) => visitor.visit_string(utf8),
                    Err(e) => visitor.visit_byte_buf(e.into_bytes()),
                },
            },
            Token::BeginList => self.visit_seq_container(visitor),
            Token::BeginMap => self.visit_map_container(b'}', visitor),
            Token::BeginAttributes => self.visit_map_container(b'>', visitor),
            t => Err(YsonError::Custom(format!("Unexpected token: {t:?}"))),
        }
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let was_reading_attributes = self.is_reading_attributes;
        self.is_reading_attributes = false;

        if was_reading_attributes {
            if self.reader.peek_byte()? == b'<' {
                self.is_reading_attributes = true;
                let res = visitor.visit_some(&mut *self);
                self.is_reading_attributes = false;
                res
            } else {
                visitor.visit_none()
            }
        } else {
            self.skip_attributes()?;
            if self.reader.peek_byte()? == b'#' {
                self.reader.next_token()?;
                visitor.visit_none()
            } else {
                visitor.visit_some(self)
            }
        }
    }

    fn deserialize_struct<V>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if name == "$__yson_attributes" {
            return visitor.visit_seq(AttributesWrapperAccess::new(self)?);
        }
        // `$value` names the body just as `@x` names an attribute, so a struct
        // that has one is flattened too.
        if fields.iter().any(|f| f.starts_with('@') || *f == "$value") {
            return visitor.visit_map(FlatStructAccess::new(self)?);
        }

        if !self.is_reading_attributes {
            self.skip_attributes()?;
        }
        self.deserialize_any(visitor)
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if !self.is_reading_attributes {
            self.skip_attributes()?;
        }

        let peeked = self.reader.peek_byte()?;
        if peeked == b'{' {
            self.reader.next_token()?;
            let val = visitor.visit_enum(EnumAccess::new(self, true))?;

            loop {
                match self.reader.peek_byte() {
                    Ok(b';' | b'}') => break,
                    Ok(_) => {
                        self.reader.next_token()?;
                    }
                    Err(_) => break,
                }
            }

            if let Ok(b';') = self.reader.peek_byte() {
                self.reader.next_token()?;
            }

            match self.reader.next_token()? {
                Token::EndMap => Ok(val),
                t => Err(YsonError::Custom(format!(
                    "Expected '}}' after variant, got {t:?}"
                ))),
            }
        } else {
            visitor.visit_enum(EnumAccess::new(self, false))
        }
    }

    delegate_skip_attributes! {
        deserialize_bool, deserialize_i8, deserialize_i16, deserialize_i32,
        deserialize_i64, deserialize_i128, deserialize_u8, deserialize_u16,
        deserialize_u32, deserialize_u64, deserialize_u128, deserialize_f32,
        deserialize_f64, deserialize_char, deserialize_str, deserialize_string,
        deserialize_bytes, deserialize_byte_buf, deserialize_unit,
        deserialize_seq, deserialize_map, deserialize_identifier,
        deserialize_ignored_any
    }

    fn deserialize_unit_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if !self.is_reading_attributes {
            self.skip_attributes()?;
        }
        self.deserialize_any(visitor)
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if !self.is_reading_attributes {
            self.skip_attributes()?;
        }
        self.deserialize_any(visitor)
    }

    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if !self.is_reading_attributes {
            self.skip_attributes()?;
        }
        self.deserialize_any(visitor)
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if !self.is_reading_attributes {
            self.skip_attributes()?;
        }
        self.deserialize_any(visitor)
    }
}
