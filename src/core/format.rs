/// Which of the two YSON encodings is in play.
///
/// Both spell the structural characters (`< > [ ] { } ; = #`) as the same ASCII
/// bytes; they differ in how scalars and strings are written.
///
/// # Examples
///
/// ```
/// use yson_rs::{Writer, YsonFormat};
///
/// let mut text = Vec::new();
/// Writer::new(&mut text, YsonFormat::Text).write_u64(42);
/// assert_eq!(text, b"42u");
///
/// let mut binary = Vec::new();
/// Writer::new(&mut binary, YsonFormat::Binary).write_u64(42);
/// assert_eq!(binary, [0x06, 42]);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum YsonFormat {
    /// Length-prefixed and type-tagged. What a job's data stream uses.
    Binary,
    /// Human-readable, with comments and escapes.
    Text,
}

impl YsonFormat {
    /// Returns `true` for [`YsonFormat::Binary`].
    ///
    /// ```
    /// use yson_rs::YsonFormat;
    ///
    /// assert!(YsonFormat::Binary.is_binary());
    /// assert!(!YsonFormat::Text.is_binary());
    /// ```
    #[must_use]
    pub const fn is_binary(self) -> bool {
        matches!(self, YsonFormat::Binary)
    }
}
