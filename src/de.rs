use std::borrow::Cow;

use crate::error::YsonError;
use crate::lexer::YsonIterator;
use crate::node::Token;
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};

pub struct Deserializer<'de> {
    pub lexer: YsonIterator<'de>,
    pub is_reading_attributes: bool,
}

impl<'de> Deserializer<'de> {
    pub fn from_bytes(input: &'de [u8], is_binary: bool) -> Self {
        Deserializer {
            lexer: YsonIterator::new(input, is_binary),
            is_reading_attributes: false,
        }
    }

    fn skip_attributes(&mut self) -> Result<(), YsonError> {
        if self.lexer.peek()? == &Token::BeginAttributes {
            self.lexer.next_token()?;
            let mut depth = 1;
            while depth > 0 {
                match self.lexer.next_token()? {
                    Token::BeginAttributes => depth += 1,
                    Token::EndAttributes => depth -= 1,
                    _ => {}
                }
            }
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
            if self.lexer.peek()? != &Token::BeginAttributes {
                return visitor.visit_map(EmptyMapAccess);
            }
            self.lexer.next_token()?;
            return visitor.visit_map(CommaSeparated::new(self, Token::EndAttributes));
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
            Token::BeginList => visitor.visit_seq(CommaSeparated::new(self, Token::EndList)),
            Token::BeginMap => visitor.visit_map(CommaSeparated::new(self, Token::EndMap)),
            Token::BeginAttributes => {
                visitor.visit_map(CommaSeparated::new(self, Token::EndAttributes))
            }
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
            if self.lexer.peek()? == &Token::BeginAttributes {
                self.is_reading_attributes = true;
                let res = visitor.visit_some(&mut *self);
                self.is_reading_attributes = false;
                res
            } else {
                visitor.visit_none()
            }
        } else {
            self.skip_attributes()?;
            if self.lexer.peek()? == &Token::Entity {
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
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if name == "$__yson_attributes" {
            return visitor.visit_seq(AttributesWrapperAccess { de: self, state: 0 });
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
    end_token: Token<'static>,
}

impl<'a, 'de> CommaSeparated<'a, 'de> {
    fn new(de: &'a mut Deserializer<'de>, end_token: Token<'static>) -> Self {
        CommaSeparated { de, end_token }
    }
}

impl<'de, 'a> SeqAccess<'de> for CommaSeparated<'a, 'de> {
    type Error = YsonError;
    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        if self.de.lexer.peek()? == &self.end_token {
            self.de.lexer.next_token()?;
            return Ok(None);
        }
        let value = seed.deserialize(&mut *self.de)?;
        if self.de.lexer.peek()? == &Token::ItemSeparator {
            self.de.lexer.next_token()?;
        }
        Ok(Some(value))
    }
}

impl<'de, 'a> MapAccess<'de> for CommaSeparated<'a, 'de> {
    type Error = YsonError;
    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>,
    {
        if self.de.lexer.peek()? == &self.end_token {
            self.de.lexer.next_token()?;
            return Ok(None);
        }
        seed.deserialize(&mut *self.de).map(Some)
    }
    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        if self.de.lexer.next_token()? != Token::KeyValueSeparator {
            return Err(YsonError::Custom("Expected '='".into()));
        }
        let value = seed.deserialize(&mut *self.de)?;
        if self.de.lexer.peek()? == &Token::ItemSeparator {
            self.de.lexer.next_token()?;
        }
        Ok(value)
    }
}
