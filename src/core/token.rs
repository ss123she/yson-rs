use std::borrow::Cow;

/// A lexical unit produced by [`crate::Reader`].
///
/// A string token borrows from the input when it can: an unquoted or
/// escape-free literal is [`Cow::Borrowed`], and only one carrying escapes
/// is [`Cow::Owned`]. Keep that split when adding a variant.
#[derive(Debug, Clone, PartialEq)]
pub enum Token<'a> {
    /// Opening bracket for attributes: `<`.
    BeginAttributes,
    /// Closing bracket for attributes: `>`.
    EndAttributes,
    /// Opening bracket for a list: `[`.
    BeginList,
    /// Closing bracket for a list: `]`.
    EndList,
    /// Opening bracket for a map: `{`.
    BeginMap,
    /// Closing bracket for a map: `}`.
    EndMap,

    /// A string literal, either quoted or unquoted. Uses `Cow` for zero-copy borrowing.
    String(Cow<'a, [u8]>),
    /// A signed 64-bit integer literal.
    Int64(i64),
    /// An unsigned 64-bit integer literal.
    Uint64(u64),
    /// A floating point literal.
    Double(f64),
    /// A boolean literal.
    Boolean(bool),
    /// The entity literal: `#`.
    Entity,

    /// Key-value separator: `=`.
    KeyValueSeparator,
    /// Item separator: `;`.
    ItemSeparator,
}

impl Token<'_> {
    /// The token's shape, without its payload.
    #[must_use]
    pub fn kind(&self) -> TokenKind {
        match self {
            Token::BeginAttributes => TokenKind::BeginAttributes,
            Token::EndAttributes => TokenKind::EndAttributes,
            Token::BeginList => TokenKind::BeginList,
            Token::EndList => TokenKind::EndList,
            Token::BeginMap => TokenKind::BeginMap,
            Token::EndMap => TokenKind::EndMap,
            Token::String(_) => TokenKind::String,
            Token::Int64(_) => TokenKind::Int64,
            Token::Uint64(_) => TokenKind::Uint64,
            Token::Double(_) => TokenKind::Double,
            Token::Boolean(_) => TokenKind::Boolean,
            Token::Entity => TokenKind::Entity,
            Token::KeyValueSeparator => TokenKind::KeyValueSeparator,
            Token::ItemSeparator => TokenKind::ItemSeparator,
        }
    }
}

/// A [`Token`] with its payload dropped: the shape alone.
///
/// What [`crate::Reader::skip_token`] reports. A type that cannot carry a
/// value cannot allocate one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    /// `<`
    BeginAttributes,
    /// `>`
    EndAttributes,
    /// `[`
    BeginList,
    /// `]`
    EndList,
    /// `{`
    BeginMap,
    /// `}`
    EndMap,
    /// A string literal.
    String,
    /// A signed 64-bit integer literal.
    Int64,
    /// An unsigned 64-bit integer literal.
    Uint64,
    /// A floating point literal.
    Double,
    /// A boolean literal.
    Boolean,
    /// `#`
    Entity,
    /// `=`
    KeyValueSeparator,
    /// `;`
    ItemSeparator,
}
