use thiserror::Error;

/// Errors that can occur during YSON serialization or deserialization.
#[derive(Error, Clone, Debug, PartialEq)]
pub enum YsonError {
    /// Reached the end of the input stream gracefully.
    #[error("End of input")]
    Eof,

    /// Reached the end of the input unexpectedly (e.g., in the middle of a string).
    /// Contains the byte position where the EOF was encountered.
    #[error("Unexpected end of input at position {0}")]
    UnexpectedEof(usize),

    /// Encountered a byte that is not a valid YSON marker.
    /// Contains the invalid byte and its position.
    #[error("Invalid binary marker 0x{0:x} at position {1}")]
    InvalidMarker(u8, usize),

    /// A catch-all for custom errors produced by `serde` or the user's data types.
    #[error("Custom error from serde: {0}")]
    Custom(String),
}

#[cfg(feature = "serde")]
mod serde_impls {
    use super::YsonError;
    use std::fmt::Display;

    impl serde::de::Error for YsonError {
        fn custom<T: Display>(msg: T) -> Self {
            YsonError::Custom(msg.to_string())
        }
    }

    impl serde::ser::Error for YsonError {
        fn custom<T: Display>(msg: T) -> Self {
            YsonError::Custom(msg.to_string())
        }
    }
}
