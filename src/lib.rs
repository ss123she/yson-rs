pub mod access;
pub mod attributes;
pub mod de;
pub mod error;
pub mod lexer;
pub mod node;
pub mod ser;
pub mod varint;

pub use crate::attributes::WithAttributes;
pub use crate::de::StreamDeserializer;
pub use crate::error::YsonError;
pub use crate::node::{YsonNode, YsonValue};
pub use crate::ser::YsonFormat;

use crate::de::Deserializer;
use crate::ser::Serializer;
use serde::{Deserialize, Serialize};

fn is_binary(format: YsonFormat) -> bool {
    match format {
        YsonFormat::Binary => true,
        YsonFormat::Text => false,
    }
}

/// Deserializes an instance of type `T` from a byte slice in the specified YSON format.
pub fn from_slice<'a, T>(bytes: &'a [u8], format: YsonFormat) -> Result<T, YsonError>
where
    T: Deserialize<'a>,
{
    let mut de = Deserializer::from_bytes(bytes, is_binary(format));
    T::deserialize(&mut de)
}

/// Serializes the given value into a byte vector using the specified YSON format.
pub fn to_vec<T: Serialize>(value: &T, format: YsonFormat) -> Result<Vec<u8>, YsonError> {
    let mut ser = Serializer::new(is_binary(format));
    value.serialize(&mut ser)?;
    Ok(ser.output)
}

/// Serializes the given value into a YSON string.
///
/// # Errors
/// Returns an error if the format is `Binary` or if the output contains invalid UTF-8 sequences.
pub fn to_string<T: Serialize>(value: &T, format: YsonFormat) -> Result<String, YsonError> {
    if matches!(format, YsonFormat::Binary) {
        return Err(YsonError::Custom(
            "Cannot use to_string for binary format".into(),
        ));
    }
    let bytes = to_vec(value, format)?;
    String::from_utf8(bytes).map_err(|_| YsonError::Custom("Invalid UTF-8 output".into()))
}
