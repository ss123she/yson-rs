use serde::Deserialize;

use crate::{de::Deserializer, error::YsonError};

pub mod access;
pub mod attributes;
pub mod de;
pub mod error;
pub mod lexer;
pub mod node;
pub mod parser;
pub mod ser;
pub mod varint;

pub fn from_slice<'a, T>(bytes: &'a [u8], is_binary: bool) -> Result<T, YsonError>
where
    T: Deserialize<'a>,
{
    let mut de = Deserializer::from_bytes(bytes, is_binary);
    T::deserialize(&mut de)
}
