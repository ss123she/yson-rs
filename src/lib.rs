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
///
/// # Examples
///
/// ```
/// use yson_rs::{from_slice, YsonFormat};
/// use std::collections::HashMap;
///
/// // Note: we use quotes around "42" to ensure it's parsed as a String
/// let data = b"{key=\"42\"; status=\"active\"}";
/// let map: HashMap<String, String> = from_slice(data, YsonFormat::Text).unwrap();
///
/// assert_eq!(map.get("key").unwrap(), "42");
/// assert_eq!(map.get("status").unwrap(), "active");
/// ```
pub fn from_slice<'a, T>(bytes: &'a [u8], format: YsonFormat) -> Result<T, YsonError>
where
    T: Deserialize<'a>,
{
    let mut de = Deserializer::from_bytes(bytes, is_binary(format));
    T::deserialize(&mut de)
}

/// Serializes the given value into a byte vector using the specified YSON format.
///
/// # Examples
///
/// ```
/// use yson_rs::{to_vec, YsonFormat};
///
/// let data = vec![1, 2, 3];
/// let bytes = to_vec(&data, YsonFormat::Binary).unwrap();
/// assert!(!bytes.is_empty());
/// assert_eq!(bytes[0], b'[');
/// ```
pub fn to_vec<T: Serialize>(value: &T, format: YsonFormat) -> Result<Vec<u8>, YsonError> {
    let mut ser = Serializer::new(is_binary(format));
    value.serialize(&mut ser)?;
    Ok(ser.output)
}

/// Serializes the given value into a YSON string.
///
/// # Examples
///
/// ```
/// use yson_rs::{to_string, YsonFormat};
///
/// let val = ("answer", 42);
/// let res = to_string(&val, YsonFormat::Text).unwrap();
/// assert_eq!(res, "[answer;42]");
/// ```
///
/// # Errors
///
/// Returns an error if the format is [`YsonFormat::Binary`] or if the output contains invalid UTF-8 sequences.
pub fn to_string<T: Serialize>(value: &T, format: YsonFormat) -> Result<String, YsonError> {
    if matches!(format, YsonFormat::Binary) {
        return Err(YsonError::Custom(
            "Cannot use to_string for binary format".into(),
        ));
    }
    let bytes = to_vec(value, format)?;
    String::from_utf8(bytes).map_err(|_| YsonError::Custom("Invalid UTF-8 output".into()))
}
