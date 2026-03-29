use crate::lexer::YsonIterator;
use crate::node::Token;
use crate::{access::FlatStructAccess, error::YsonError};
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use std::borrow::Cow;

pub struct Deserializer<'de> {
    pub lexer: YsonIterator<'de>,
    pub is_reading_attributes: bool,
    depth: usize,
    max_depth: usize,
}

impl<'de> Deserializer<'de> {
    pub fn from_bytes(input: &'de [u8], is_binary: bool) -> Self {
        Deserializer {
            lexer: YsonIterator::new(input, is_binary),
            is_reading_attributes: false,
            depth: 0,
            max_depth: 128,
        }
    }

    pub fn parse_t<T: de::Deserialize<'de>>(&mut self) -> Result<T, YsonError> {
        T::deserialize(self)
    }

    pub fn enter_recursion(&mut self) -> Result<(), YsonError> {
        self.depth += 1;
        if self.depth > self.max_depth {
            return Err(YsonError::Custom("Recursion limit exceeded".into()));
        }
        Ok(())
    }

    pub fn leave_recursion(&mut self) {
        self.depth -= 1;
    }

    fn skip_attributes(&mut self) -> Result<(), YsonError> {
        if self.lexer.peek_byte()? == b'<' {
            self.enter_recursion()?;
            self.lexer.next_token()?;
            let mut attr_depth = 1;
            while attr_depth > 0 {
                match self.lexer.next_token()? {
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

impl<'de> de::Deserializer<'de> for &mut Deserializer<'de> {
    type Error = YsonError;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let was_reading_attributes = self.is_reading_attributes;
        self.is_reading_attributes = false;

        if was_reading_attributes {
            if self.lexer.peek_byte()? != b'<' {
                return visitor.visit_map(EmptyMapAccess);
            }
            self.lexer.next_token()?;
            return visitor.visit_map(CommaSeparated::new(self, b'>')?);
        }

        self.skip_attributes()?;

        match self.lexer.next_token()? {
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
            Token::BeginList => visitor.visit_seq(CommaSeparated::new(self, b']')?),
            Token::BeginMap => visitor.visit_map(CommaSeparated::new(self, b'}')?),
            Token::BeginAttributes => visitor.visit_map(CommaSeparated::new(self, b'>')?),
            t => Err(YsonError::Custom(format!("Unexpected token: {:?}", t))),
        }
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let was_reading_attributes = self.is_reading_attributes;
        self.is_reading_attributes = false;

        if was_reading_attributes {
            if self.lexer.peek_byte()? == b'<' {
                self.is_reading_attributes = true;
                let res = visitor.visit_some(&mut *self);
                self.is_reading_attributes = false;
                res
            } else {
                visitor.visit_none()
            }
        } else {
            self.skip_attributes()?;
            if self.lexer.peek_byte()? == b'#' {
                self.lexer.next_token()?;
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
        if fields.iter().any(|f| f.starts_with('@')) {
            return visitor.visit_map(FlatStructAccess::new(self)?);
        }

        self.deserialize_any(visitor)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct newtype_struct seq tuple
        tuple_struct map enum identifier ignored_any
    }
}

struct EmptyMapAccess;
impl<'de> MapAccess<'de> for EmptyMapAccess {
    type Error = YsonError;
    fn next_key_seed<K>(&mut self, _seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>,
    {
        Ok(None)
    }
    fn next_value_seed<V>(&mut self, _seed: V) -> Result<V::Value, Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        unreachable!()
    }
}

struct AttributesWrapperAccess<'a, 'de: 'a> {
    de: &'a mut Deserializer<'de>,
    state: u8,
}

impl<'a, 'de> AttributesWrapperAccess<'a, 'de> {
    fn new(de: &'a mut Deserializer<'de>) -> Result<Self, YsonError> {
        de.enter_recursion()?;
        Ok(AttributesWrapperAccess { de, state: 0 })
    }
}

impl<'a, 'de> Drop for AttributesWrapperAccess<'a, 'de> {
    fn drop(&mut self) {
        self.de.leave_recursion();
    }
}

impl<'de, 'a> SeqAccess<'de> for AttributesWrapperAccess<'a, 'de> {
    type Error = YsonError;
    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        match self.state {
            0 => {
                self.state = 1;
                self.de.is_reading_attributes = true;
                let val = seed.deserialize(&mut *self.de)?;
                self.de.is_reading_attributes = false;
                Ok(Some(val))
            }
            1 => {
                self.state = 2;
                let val = seed.deserialize(&mut *self.de)?;
                Ok(Some(val))
            }
            _ => Ok(None),
        }
    }
}

struct CommaSeparated<'a, 'de: 'a> {
    de: &'a mut Deserializer<'de>,
    end_byte: u8,
}

impl<'a, 'de> CommaSeparated<'a, 'de> {
    fn new(de: &'a mut Deserializer<'de>, end_byte: u8) -> Result<Self, YsonError> {
        de.enter_recursion()?;
        Ok(CommaSeparated { de, end_byte })
    }
}

impl<'a, 'de> Drop for CommaSeparated<'a, 'de> {
    fn drop(&mut self) {
        self.de.leave_recursion();
    }
}

impl<'de, 'a> MapAccess<'de> for CommaSeparated<'a, 'de> {
    type Error = YsonError;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>,
    {
        let peeked = self.de.lexer.peek_byte()?;
        if peeked == self.end_byte {
            self.de.lexer.next_token()?;
            return Ok(None);
        }

        if peeked == b';' {
            self.de.lexer.next_token()?;

            if self.de.lexer.peek_byte()? == self.end_byte {
                self.de.lexer.next_token()?;
                return Ok(None);
            }
        }

        seed.deserialize(&mut *self.de).map(Some)
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        let token = self.de.lexer.next_token()?;
        if token != Token::KeyValueSeparator {
            return Err(YsonError::Custom(format!("Expected '=', got {:?}", token)));
        }

        seed.deserialize(&mut *self.de)
    }
}

impl<'de, 'a> SeqAccess<'de> for CommaSeparated<'a, 'de> {
    type Error = YsonError;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        let peeked = self.de.lexer.peek_byte()?;
        if peeked == self.end_byte {
            self.de.lexer.next_token()?;
            return Ok(None);
        }

        if peeked == b';' {
            self.de.lexer.next_token()?;

            if self.de.lexer.peek_byte()? == self.end_byte {
                self.de.lexer.next_token()?;
                return Ok(None);
            }
        }

        seed.deserialize(&mut *self.de).map(Some)
    }
}

pub struct StreamDeserializer<'de, T> {
    de: Deserializer<'de>,
    first: bool,
    _marker: std::marker::PhantomData<T>,
}

impl<'de, T> StreamDeserializer<'de, T>
where
    T: de::Deserialize<'de>,
{
    pub fn new(input: &'de [u8], is_binary: bool) -> Self {
        Self {
            de: Deserializer::from_bytes(input, is_binary),
            first: true,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn next_item(&mut self) -> Result<Option<T>, YsonError> {
        let peek_res = self.de.lexer.peek_byte();

        if matches!(peek_res, Err(YsonError::Eof)) {
            return Ok(None);
        }

        let next_byte = peek_res?;

        if self.first {
            self.first = false;
        } else {
            if next_byte == b';' {
                self.de.lexer.next_token()?;
                if matches!(self.de.lexer.peek_byte(), Err(YsonError::Eof)) {
                    return Ok(None);
                }
            }
        }

        let item = T::deserialize(&mut self.de)?;
        Ok(Some(item))
    }
}
