//! # yson-rs
//!
//! A serializer and deserializer for the [YSON](https://ytsaurus.tech) format.
//!
//! ## Two layers
//!
//! ```text
//!            bytes in                              bytes out
//!               |                                      ^
//!   core .......|......................................|.........
//!               v                                      |
//!        Frames / FrameReader --> record bytes         |
//!               |                                      |
//!               v                                      |
//!            Reader --> Token ------.           .--> Writer
//!               |                   |           |
//!               |                   v           |
//!               |          YsonValue<'a> -------'      scan_value
//!               |          (borrows the input)         (no values
//!               '--> skip_token --> TokenKind            at all)
//!
//!   surface ..................................................
//!
//!            Deserializer  <---- serde ---->  Serializer
//!                  |                               |
//!            from_slice                     to_vec / to_string
//! ```
//!
//! - [`core`] — the format, with no serde and no `str`. Builds with
//!   `default-features = false`.
//! - [`surface`] — the serde binding, behind the default-on **`serde`**
//!   feature. A struct field renamed `@name` is an attribute; one renamed
//!   `$value` is the body.
//!
//! ## The tree borrows its input
//!
//! Every byte string in a [`YsonValue`] is a `Cow` over the bytes it was read
//! from, so reading copies no payload. Two cases cannot borrow and do allocate:
//! a text string carrying backslash escapes, and a value built by hand. Call
//! [`YsonValue::into_owned`] to detach a tree from its buffer.
//!
//! ```
//! use yson_rs::{Reader, Writer, YsonFormat};
//!
//! let input = b"<schema=strict>{host=\"a.example\"}";
//! let value = Reader::new(input, YsonFormat::Text).read_value().unwrap();
//!
//! assert_eq!(value["@schema"].as_str(), Some("strict"));
//!
//! // The string is the input's bytes, not a copy of them.
//! let host = value["host"].as_bytes().unwrap();
//! assert!(input.as_ptr_range().contains(&host.as_ptr()));
//!
//! let mut buf = Vec::new();
//! Writer::new(&mut buf, YsonFormat::Binary).write_value(&value).unwrap();
//! ```
//!
//! ## Reading a stream
//!
//! A YTsaurus job reads a list fragment — `value; value; value` — that can be
//! larger than memory. [`Frames`] cuts one up in place; [`FrameReader`] pulls
//! from anything [`Read`](std::io::Read).
//!
//! Framing is `core`, so it needs no serde — a job that forwards rows never
//! decodes one:
//!
//! ```
//! use yson_rs::{Frames, Reader, YsonFormat};
//!
//! let mut total = 0;
//! for frame in Frames::new(b"{n=1};{n=2}", YsonFormat::Text) {
//!     let row = Reader::new(frame.unwrap(), YsonFormat::Text).read_value().unwrap();
//!     total += row["n"].as_i64().unwrap();
//! }
//! assert_eq!(total, 3);
//! ```

// The typed example needs the feature, so it is only compiled -- and only
// doc-tested -- when the feature is on.
#![cfg_attr(
    feature = "serde",
    doc = r#"
## Typed, through serde

```
use yson_rs::{YsonFormat, from_slice, to_string};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct User {
    name: String,
    age: u32,
}

let user = User { name: "Alice".into(), age: 42 };
let text = to_string(&user, YsonFormat::Text).unwrap();

assert_eq!(from_slice::<User>(text.as_bytes(), YsonFormat::Text).unwrap(), user);
```
"#
)]
#![warn(missing_docs)]

pub mod core;

#[cfg(feature = "serde")]
pub mod surface;

pub use crate::core::{
    FrameReader, Frames, OwnedYsonValue, Reader, Scan, Token, TokenKind, Writer, YsonError,
    YsonFormat, YsonKey, YsonMap, YsonNode, YsonValue, scan_value,
};

#[cfg(feature = "serde")]
pub use crate::surface::{attributes::WithAttributes, de::Deserializer, ser::Serializer};

#[cfg(feature = "serde")]
mod convenience {
    use crate::core::{YsonError, YsonFormat};
    use crate::surface::{de::Deserializer, ser::Serializer};
    use serde::{Deserialize, Serialize};

    /// Deserializes an instance of type `T` from a byte slice in the specified YSON format.
    ///
    /// The slice must hold **one** value and nothing else. Trailing whitespace
    /// and comments are fine in text mode; anything else is an error, so a
    /// truncated or concatenated document is not mistaken for a healthy one.
    /// A sequence of values is framed with [`crate::scan_value`].
    ///
    /// # Examples
    ///
    /// ```
    /// use yson_rs::{from_slice, YsonFormat};
    /// use std::collections::HashMap;
    ///
    /// let data = b"{key=\"42\"; status=\"active\"}";
    /// let map: HashMap<String, String> = from_slice(data, YsonFormat::Text).unwrap();
    ///
    /// assert_eq!(map.get("key").unwrap(), "42");
    ///
    /// // The whole slice has to be the value:
    /// assert!(from_slice::<i64>(b"42 garbage", YsonFormat::Text).is_err());
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`YsonError`] if:
    /// - The input data has invalid YSON syntax.
    /// - The input contains invalid UTF-8 sequences (when in text mode).
    /// - The data structure does not match the requirements of the target type `T`.
    /// - Anything follows the value, in which case the error names the offset.
    pub fn from_slice<'a, T>(bytes: &'a [u8], format: YsonFormat) -> Result<T, YsonError>
    where
        T: Deserialize<'a>,
    {
        let mut de = Deserializer::new(bytes, format);
        let value = T::deserialize(&mut de)?;
        de.end()?;
        Ok(value)
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
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`YsonError`] if serialization fails, which can occur due to:
    /// - Recursion depth limits being exceeded.
    /// - Custom serialization errors defined by the type `T`.
    pub fn to_vec<T: Serialize>(value: &T, format: YsonFormat) -> Result<Vec<u8>, YsonError> {
        let mut ser = Serializer::new(format);
        value.serialize(&mut ser)?;
        Ok(ser.output)
    }

    /// Serializes the given value into a YSON-formatted string.
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
    /// Returns an error if:
    /// - The format is [`YsonFormat::Binary`] (binary YSON cannot be represented as a UTF-8 string).
    /// - The serialization output contains invalid UTF-8 sequences.
    /// - Serialization fails due to internal structural constraints.
    pub fn to_string<T: Serialize>(value: &T, format: YsonFormat) -> Result<String, YsonError> {
        if format.is_binary() {
            return Err(YsonError::Custom(
                "Cannot use to_string for binary format".into(),
            ));
        }
        let bytes = to_vec(value, format)?;
        String::from_utf8(bytes).map_err(|_| YsonError::Custom("Invalid UTF-8 output".into()))
    }
}

#[cfg(feature = "serde")]
pub use convenience::{from_slice, to_string, to_vec};
