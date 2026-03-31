use crate::access::{AttributesWrapperAccess, CommaSeparated, EmptyMapAccess, EnumAccess};
use crate::lexer::YsonIterator;
use crate::node::{Token, YsonNode, YsonValue};
use crate::{access::FlatStructAccess, error::YsonError};
use serde::Deserialize;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use std::borrow::Cow;
use std::collections::BTreeMap;

pub struct Deserializer<'de> {
    pub(crate) lexer: YsonIterator<'de>,
    pub(crate) is_reading_attributes: bool,
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

        if self.lexer.peek_byte()? == b'<' {
            return visitor.visit_map(FlatStructAccess::new(self)?);
        }

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

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.skip_attributes()?;
        let peeked = self.lexer.peek_byte()?;
        if peeked == b'{' {
            self.lexer.next_token()?;
            let val = visitor.visit_enum(EnumAccess::new(self, true))?;

            loop {
                match self.lexer.peek_byte() {
                    Ok(b';') | Ok(b'}') => break,
                    Ok(_) => {
                        self.lexer.next_token()?;
                    }
                    Err(_) => break,
                }
            }

            if let Ok(b';') = self.lexer.peek_byte() {
                self.lexer.next_token()?;
            }

            match self.lexer.next_token()? {
                Token::EndMap => Ok(val),
                t => Err(YsonError::Custom(format!(
                    "Expected '}}' after variant, got {:?}",
                    t
                ))),
            }
        } else {
            visitor.visit_enum(EnumAccess::new(self, false))
        }
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct newtype_struct seq tuple
        tuple_struct map identifier ignored_any
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

macro_rules! impl_visit_primitives {
    ( $( $method:ident ( $v_type:ty ) => $node_variant:ident ),* ) => {
        $(
            fn $method<E>(self, v: $v_type) -> Result<Self::Value, E> {
                Ok(YsonValue {
                    attributes: None,
                    node: YsonNode::$node_variant(v),
                })
            }
        )*
    };
}

impl<'de> Deserialize<'de> for YsonValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct YsonValueVisitor;

        impl<'de> Visitor<'de> for YsonValueVisitor {
            type Value = YsonValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("any YSON value")
            }

            impl_visit_primitives! {
                visit_bool(bool) => Boolean,
                visit_i64(i64) => Int64,
                visit_u64(u64) => Uint64,
                visit_f64(f64) => Double
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(YsonValue {
                    attributes: None,
                    node: YsonNode::String(v.as_bytes().to_vec()),
                })
            }

            fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
                Ok(YsonValue {
                    attributes: None,
                    node: YsonNode::String(v.to_vec()),
                })
            }

            fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<Self::Value, E> {
                Ok(YsonValue {
                    attributes: None,
                    node: YsonNode::String(v),
                })
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(YsonValue {
                    attributes: None,
                    node: YsonNode::Entity,
                })
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut vec = Vec::new();
                while let Some(elem) = seq.next_element()? {
                    vec.push(elem);
                }
                Ok(YsonValue {
                    attributes: None,
                    node: YsonNode::List(vec),
                })
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut attributes = BTreeMap::new();
                let mut plain_map = BTreeMap::new();
                let mut body_node = None;
                let mut is_attributed = false;

                while let Some(key) = map.next_key::<String>()? {
                    if let Some(attr_name) = key.strip_prefix('@') {
                        is_attributed = true;
                        attributes.insert(attr_name.as_bytes().to_vec(), map.next_value()?);
                    } else if key == "$value" {
                        is_attributed = true;
                        let val: YsonValue = map.next_value()?;
                        body_node = Some(val.node);
                        if let Some(inner_attrs) = val.attributes {
                            attributes.extend(inner_attrs);
                        }
                    } else {
                        plain_map.insert(key.into_bytes(), map.next_value()?);
                    }
                }

                if is_attributed {
                    Ok(YsonValue {
                        attributes: if attributes.is_empty() {
                            None
                        } else {
                            Some(attributes)
                        },
                        node: body_node.unwrap_or(YsonNode::Entity),
                    })
                } else {
                    Ok(YsonValue {
                        attributes: None,
                        node: YsonNode::Map(plain_map),
                    })
                }
            }
        }

        deserializer.deserialize_any(YsonValueVisitor)
    }
}
