use serde::de::{
    self, DeserializeSeed, IntoDeserializer, MapAccess, Visitor, value::StringDeserializer,
};
use std::borrow::Cow;

use crate::{de::Deserializer, error::YsonError, node::Token};

#[derive(PartialEq)]
enum FlatState {
    Attributes,
    Between,
    Body,
    ValueOnly,
    Done,
}

pub struct FlatStructAccess<'a, 'de: 'a> {
    de: &'a mut Deserializer<'de>,
    state: FlatState,
    is_value_only: bool,
}

impl<'a, 'de> FlatStructAccess<'a, 'de> {
    pub fn new(de: &'a mut Deserializer<'de>) -> Result<Self, YsonError> {
        de.enter_recursion()?;

        let state = match de.lexer.peek_byte()? {
            b'<' => {
                de.lexer.next_token()?;
                FlatState::Attributes
            }
            b'{' => {
                de.lexer.next_token()?;
                FlatState::Body
            }
            b'#' => {
                de.lexer.next_token()?;
                FlatState::Done
            }
            _ => FlatState::ValueOnly,
        };

        Ok(FlatStructAccess {
            de,
            state,
            is_value_only: false,
        })
    }
}

impl<'a, 'de> Drop for FlatStructAccess<'a, 'de> {
    fn drop(&mut self) {
        self.de.leave_recursion();
    }
}

impl<'de, 'a> MapAccess<'de> for FlatStructAccess<'a, 'de> {
    type Error = YsonError;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>,
    {
        loop {
            match self.state {
                FlatState::Attributes => {
                    let peeked = self.de.lexer.peek_byte()?;
                    if peeked == b'>' {
                        self.de.lexer.next_token()?;
                        self.state = FlatState::Between;
                        continue;
                    }
                    if peeked == b';' {
                        self.de.lexer.next_token()?;
                        continue;
                    }

                    let token = self.de.lexer.next_token()?;
                    if let Token::String(s) = token {
                        let key_str = match &s {
                            Cow::Borrowed(b) => std::str::from_utf8(b).unwrap_or(""),
                            Cow::Owned(vec) => std::str::from_utf8(vec).unwrap_or(""),
                        };
                        let prefixed = format!("@{}", key_str);
                        let deserializer: StringDeserializer<YsonError> =
                            prefixed.into_deserializer();
                        self.is_value_only = false;
                        return seed.deserialize(deserializer).map(Some);
                    } else {
                        return Err(YsonError::Custom(
                            "Expected string key in attributes".into(),
                        ));
                    }
                }
                FlatState::Between => {
                    let peeked = self.de.lexer.peek_byte()?;
                    if peeked == b'{' {
                        self.de.lexer.next_token()?;
                        self.state = FlatState::Body;
                        continue;
                    } else if peeked == b'#' {
                        self.de.lexer.next_token()?;
                        self.state = FlatState::Done;
                        return Ok(None);
                    } else {
                        self.state = FlatState::ValueOnly;
                        continue;
                    }
                }
                FlatState::Body => {
                    let peeked = self.de.lexer.peek_byte()?;
                    if peeked == b'}' {
                        self.de.lexer.next_token()?;
                        self.state = FlatState::Done;
                        return Ok(None);
                    }
                    if peeked == b';' {
                        self.de.lexer.next_token()?;
                        continue;
                    }

                    self.is_value_only = false;
                    return seed.deserialize(&mut *self.de).map(Some);
                }
                FlatState::ValueOnly => {
                    self.state = FlatState::Done;
                    self.is_value_only = true;
                    let deserializer: StringDeserializer<YsonError> =
                        "$value".to_string().into_deserializer();
                    return seed.deserialize(deserializer).map(Some);
                }
                FlatState::Done => return Ok(None),
            }
        }
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        if self.is_value_only {
            return seed.deserialize(&mut *self.de);
        }

        let token = self.de.lexer.next_token()?;
        if token != Token::KeyValueSeparator {
            return Err(YsonError::Custom(format!("Expected '=', got {:?}", token)));
        }
        seed.deserialize(&mut *self.de)
    }
}

pub(crate) struct EnumAccess<'a, 'de: 'a> {
    de: &'a mut Deserializer<'de>,
    is_map_wrapped: bool,
}

impl<'a, 'de> EnumAccess<'a, 'de> {
    pub(crate) fn new(de: &'a mut Deserializer<'de>, is_map_wrapped: bool) -> Self {
        EnumAccess { de, is_map_wrapped }
    }
}

impl<'de, 'a> de::EnumAccess<'de> for EnumAccess<'a, 'de> {
    type Error = YsonError;
    type Variant = Self;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error>
    where
        V: de::DeserializeSeed<'de>,
    {
        let val = seed.deserialize(&mut *self.de)?;
        Ok((val, self))
    }
}

impl<'de, 'a> de::VariantAccess<'de> for EnumAccess<'a, 'de> {
    type Error = YsonError;

    fn unit_variant(self) -> Result<(), Self::Error> {
        if self.is_map_wrapped {
            let token = self.de.lexer.next_token()?;
            if token != Token::KeyValueSeparator {
                return Err(YsonError::Custom("Expected '='".into()));
            }
            let val_token = self.de.lexer.next_token()?;
            if val_token != Token::Entity {
                return Err(YsonError::Custom(
                    "Expected '#' for unit variant in map".into(),
                ));
            }
        }
        Ok(())
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        let token = self.de.lexer.next_token()?;
        if token != Token::KeyValueSeparator {
            return Err(YsonError::Custom("Expected '='".into()));
        }
        seed.deserialize(self.de)
    }

    fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let token = self.de.lexer.next_token()?;
        if token != Token::KeyValueSeparator {
            return Err(YsonError::Custom("Expected '='".into()));
        }
        de::Deserializer::deserialize_seq(self.de, visitor)
    }

    fn struct_variant<V>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let token = self.de.lexer.next_token()?;
        if token != Token::KeyValueSeparator {
            return Err(YsonError::Custom("Expected '='".into()));
        }
        de::Deserializer::deserialize_map(self.de, visitor)
    }
}
