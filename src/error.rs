use std::fmt::Display;

use serde::de;
use thiserror::Error;

#[derive(Error, Clone, Debug, PartialEq)]
pub enum YsonError {
    #[error("End of input")]
    Eof,

    #[error("Unexpected end of input at position {0}")]
    UnexpectedEof(usize),

    #[error("Invalid binary marker 0x{0:x} at position {1}")]
    InvalidMarker(u8, usize),

    #[error("Malformed varint at position {0}")]
    MalformedVarint(usize),

    #[error("Invalid UTF-8 string at position {0}")]
    InvalidUtf8(usize),

    #[error("Expected {expected}, found {found} at position {pos}")]
    UnexpectedToken {
        expected: &'static str,
        found: String,
        pos: usize,
    },

    #[error("Custom error from serde: {0}")]
    Custom(String),
}

impl de::Error for YsonError {
    fn custom<T: Display>(msg: T) -> Self {
        YsonError::Custom(msg.to_string())
    }
}

impl serde::ser::Error for YsonError {
    fn custom<T: Display>(msg: T) -> Self {
        YsonError::Custom(msg.to_string())
    }
}
