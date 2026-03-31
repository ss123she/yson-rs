use std::{borrow::Cow, collections::BTreeMap};

/// Represents a complete YSON value, including its optional attributes and data node.
#[derive(Debug, Clone, PartialEq)]
pub struct YsonValue {
    /// Optional attributes associated with this value.
    /// In YSON, attributes are stored as a map of byte strings to other YSON values.
    pub attributes: Option<BTreeMap<Vec<u8>, YsonValue>>,
    /// The data content of this YSON node.
    pub node: YsonNode,
}

impl YsonValue {
    /// Attempts to interpret the node as a UTF-8 string.
    /// Returns `None` if the node is not a string or if the bytes are not valid UTF-8.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        if let YsonNode::String(bytes) = &self.node {
            std::str::from_utf8(bytes).ok()
        } else {
            None
        }
    }

    /// Attempts to interpret the node as a 64-bit signed integer.
    /// Returns `None` if the node is not an `Int64`.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        if let YsonNode::Int64(v) = self.node {
            Some(v)
        } else {
            None
        }
    }

    /// Retrieves an attribute by its string key.
    /// Returns `None` if attributes are missing or the key is not found.
    #[must_use]
    pub fn attr(&self, key: &str) -> Option<&YsonValue> {
        self.attributes.as_ref()?.get(key.as_bytes())
    }
}

impl<'a> std::ops::Index<&'a str> for YsonValue {
    type Output = YsonValue;

    /// Provides convenient access to map elements or attributes using index notation.
    ///
    /// # Panics
    /// Panics if the key is not found or if the value is not a map.
    ///
    /// # Examples
    /// ```
    /// use yson_rs::{YsonValue, from_slice, YsonFormat};
    ///
    /// let input = b"<status=\"ok\">{id=1u}";
    /// let value: YsonValue = from_slice(input, YsonFormat::Text).unwrap();
    ///
    /// // Access an attribute with '@' prefix
    /// assert_eq!(value["@status"].as_str(), Some("ok"));
    ///
    /// // Access a map field directly
    /// // Note: value["id"] would work if it were a map
    /// ```
    fn index(&self, key: &'a str) -> &Self::Output {
        if let Some(attr_name) = key.strip_prefix('@') {
            return self
                .attributes
                .as_ref()
                .and_then(|a| a.get(attr_name.as_bytes()))
                .expect("Attribute not found");
        }
        if let YsonNode::Map(m) = &self.node {
            return m.get(key.as_bytes()).expect("Key not found in map");
        }
        panic!("Value is not a map");
    }
}

/// Represents the data variants available in the YSON data model.
#[derive(Debug, Clone, PartialEq)]
pub enum YsonNode {
    /// An empty value, represented by `#` in text format.
    Entity,
    /// A boolean value (`%true` or `%false`).
    Boolean(bool),
    /// A signed 64-bit integer.
    Int64(i64),
    /// An unsigned 64-bit integer, followed by `u` in text format (e.g., `42u`).
    Uint64(u64),
    /// A double-precision floating point number.
    Double(f64),
    /// A byte string.
    String(Vec<u8>),
    /// A list of YSON values, enclosed in `[...]`.
    List(Vec<YsonValue>),
    /// A map of byte strings to YSON values, enclosed in `{...}`.
    Map(BTreeMap<Vec<u8>, YsonValue>),
}

/// Represents individual lexical units (tokens) produced by the YSON lexer.
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
