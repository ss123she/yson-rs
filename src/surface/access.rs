use serde::de::{
    self, DeserializeSeed, IntoDeserializer, MapAccess, SeqAccess, Visitor,
    value::StringDeserializer,
};

use crate::core::error::YsonError;
use crate::core::token::Token;
use crate::surface::de::Deserializer;

/// Hands a map key to a visitor as raw bytes, with no UTF-8 validation.
struct ByteKeyDeserializer(Vec<u8>);

impl<'de> de::Deserializer<'de> for ByteKeyDeserializer {
    type Error = YsonError;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_byte_buf(self.0)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

#[derive(PartialEq)]
enum FlatState {
    Attributes,
    Between,
    Body,
    ValueOnly,
    Done,
}

pub(crate) struct FlatStructAccess<'a, 'de: 'a> {
    de: &'a mut Deserializer<'de>,
    state: FlatState,
    is_value_only: bool,
}

impl<'a, 'de> FlatStructAccess<'a, 'de> {
    pub(crate) fn new(de: &'a mut Deserializer<'de>) -> Result<Self, YsonError> {
        de.enter_recursion()?;

        let state = match de.reader.peek_byte()? {
            b'<' => {
                de.reader.next_token()?;
                FlatState::Attributes
            }
            b'{' => {
                de.reader.next_token()?;
                FlatState::Body
            }
            b'#' => {
                de.reader.next_token()?;
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

impl Drop for FlatStructAccess<'_, '_> {
    fn drop(&mut self) {
        self.de.leave_recursion();
    }
}

impl<'de> MapAccess<'de> for FlatStructAccess<'_, 'de> {
    type Error = YsonError;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>,
    {
        loop {
            match self.state {
                FlatState::Attributes => {
                    let peeked = self.de.reader.peek_byte()?;
                    if peeked == b'>' {
                        self.de.reader.next_token()?;
                        self.state = FlatState::Between;
                        continue;
                    }
                    if peeked == b';' {
                        self.de.reader.next_token()?;
                        continue;
                    }

                    let token = self.de.reader.next_token()?;
                    if let Token::String(s) = token {
                        // Attribute names are arbitrary byte strings, so the key goes to
                        // the visitor as bytes. `#[serde(rename = "@name")]` still
                        // matches: derived field identifiers implement `visit_bytes`.
                        let mut prefixed = Vec::with_capacity(s.len() + 1);
                        prefixed.push(b'@');
                        prefixed.extend_from_slice(&s);
                        self.is_value_only = false;
                        return seed.deserialize(ByteKeyDeserializer(prefixed)).map(Some);
                    }
                    return Err(YsonError::Custom(
                        "Expected string key in attributes".into(),
                    ));
                }
                FlatState::Between => {
                    let peeked = self.de.reader.peek_byte()?;
                    if peeked == b'{' {
                        self.de.reader.next_token()?;
                        self.state = FlatState::Body;
                        continue;
                    } else if peeked == b'#' {
                        self.de.reader.next_token()?;
                        self.state = FlatState::Done;
                        return Ok(None);
                    }
                    self.state = FlatState::ValueOnly;
                    continue;
                }
                FlatState::Body => {
                    let peeked = self.de.reader.peek_byte()?;
                    if peeked == b'}' {
                        self.de.reader.next_token()?;
                        self.state = FlatState::Done;
                        return Ok(None);
                    }
                    if peeked == b';' {
                        self.de.reader.next_token()?;
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

        let token = self.de.reader.next_token()?;
        if token != Token::KeyValueSeparator {
            return Err(YsonError::Custom(format!("Expected '=', got {token:?}")));
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

impl<'de> de::EnumAccess<'de> for EnumAccess<'_, 'de> {
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

impl<'de> de::VariantAccess<'de> for EnumAccess<'_, 'de> {
    type Error = YsonError;

    fn unit_variant(self) -> Result<(), Self::Error> {
        if self.is_map_wrapped {
            let token = self.de.reader.next_token()?;
            if token != Token::KeyValueSeparator {
                return Err(YsonError::Custom("Expected '='".into()));
            }
            let val_token = self.de.reader.next_token()?;
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
        let token = self.de.reader.next_token()?;
        if token != Token::KeyValueSeparator {
            return Err(YsonError::Custom("Expected '='".into()));
        }
        seed.deserialize(self.de)
    }

    fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let token = self.de.reader.next_token()?;
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
        let token = self.de.reader.next_token()?;
        if token != Token::KeyValueSeparator {
            return Err(YsonError::Custom("Expected '='".into()));
        }
        de::Deserializer::deserialize_map(self.de, visitor)
    }
}

pub(crate) struct EmptyMapAccess;
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

pub(crate) struct AttributesWrapperAccess<'a, 'de: 'a> {
    de: &'a mut Deserializer<'de>,
    state: u8,
}

impl<'a, 'de> AttributesWrapperAccess<'a, 'de> {
    pub(crate) fn new(de: &'a mut Deserializer<'de>) -> Result<Self, YsonError> {
        de.enter_recursion()?;
        Ok(AttributesWrapperAccess { de, state: 0 })
    }
}

impl Drop for AttributesWrapperAccess<'_, '_> {
    fn drop(&mut self) {
        self.de.leave_recursion();
    }
}

impl<'de> SeqAccess<'de> for AttributesWrapperAccess<'_, 'de> {
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

/// Walks the items of a container the deserializer has already opened,
/// stopping at the terminator without consuming it.
///
/// Closing is the job of whoever wrote the opening token -- see
/// [`Deserializer::close_container`].
pub(crate) struct CommaSeparated<'a, 'de: 'a> {
    de: &'a mut Deserializer<'de>,
    end_byte: u8,
}

impl<'a, 'de> CommaSeparated<'a, 'de> {
    pub(crate) fn new(de: &'a mut Deserializer<'de>, end_byte: u8) -> Result<Self, YsonError> {
        de.enter_recursion()?;
        Ok(CommaSeparated { de, end_byte })
    }

    /// Positions the reader at the next item.
    ///
    /// Returns `false` at the terminator, which is left in the input.
    fn advance_to_item(&mut self) -> Result<bool, YsonError> {
        loop {
            let peeked = self.de.reader.peek_byte()?;
            if peeked == self.end_byte {
                return Ok(false);
            }
            if peeked == b';' {
                self.de.reader.next_token()?;
                continue;
            }
            return Ok(true);
        }
    }
}

impl Drop for CommaSeparated<'_, '_> {
    fn drop(&mut self) {
        self.de.leave_recursion();
    }
}

impl<'de> MapAccess<'de> for CommaSeparated<'_, 'de> {
    type Error = YsonError;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>,
    {
        if !self.advance_to_item()? {
            return Ok(None);
        }

        seed.deserialize(&mut *self.de).map(Some)
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        let token = self.de.reader.next_token()?;
        if token != Token::KeyValueSeparator {
            return Err(YsonError::Custom(format!("Expected '=', got {token:?}")));
        }

        seed.deserialize(&mut *self.de)
    }
}

impl<'de> SeqAccess<'de> for CommaSeparated<'_, 'de> {
    type Error = YsonError;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        if !self.advance_to_item()? {
            return Ok(None);
        }

        seed.deserialize(&mut *self.de).map(Some)
    }
}
