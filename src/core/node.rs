//! The YSON value tree, borrowed from the bytes it was read out of.
//!
//! Every byte string -- a string value, a map key, an attribute name -- is
//! a [`Cow`] over the input, so reading copies no
//! payload. Only two cases allocate: a text string carrying backslash
//! escapes, whose decoded bytes are nowhere in the input, and a value
//! built by hand. What still allocates is the *shape*, a `Vec` per list
//! and a `BTreeMap` per map.
//!
//! [`OwnedYsonValue`] is `YsonValue<'static>`, and
//! [`YsonValue::into_owned`] is the bridge for a caller that has to
//! outlive its buffer.

use std::borrow::Cow;
use std::collections::BTreeMap;

/// A map key or an attribute name: an arbitrary byte string.
///
/// YSON does not require keys to be UTF-8, and YTsaurus sends ones that are
/// not, so this is `[u8]` rather than `str` throughout.
pub type YsonKey<'a> = Cow<'a, [u8]>;

/// The entries of a YSON map, or of an attribute list.
pub type YsonMap<'a> = BTreeMap<YsonKey<'a>, YsonValue<'a>>;

/// A [`YsonValue`] that borrows nothing and can outlive any buffer.
pub type OwnedYsonValue = YsonValue<'static>;

/// A complete YSON value: optional attributes, and exactly one body.
#[derive(Debug, Clone, PartialEq)]
pub struct YsonValue<'a> {
    /// The attributes decorating this value, if it has any.
    ///
    /// `None` and `Some(empty)` are different documents: the first is `42`, the
    /// second is `<>42`.
    pub attributes: Option<YsonMap<'a>>,
    /// The body.
    pub node: YsonNode<'a>,
}

/// The body of a YSON value.
#[derive(Debug, Clone, PartialEq)]
pub enum YsonNode<'a> {
    /// An empty value, written `#`.
    Entity,
    /// A boolean, written `%true` or `%false`.
    Boolean(bool),
    /// A signed 64-bit integer.
    Int64(i64),
    /// An unsigned 64-bit integer, written with a `u` suffix in text format.
    Uint64(u64),
    /// A double-precision float.
    Double(f64),
    /// A byte string.
    String(Cow<'a, [u8]>),
    /// A list, written `[...]`.
    List(Vec<YsonValue<'a>>),
    /// A map, written `{...}`.
    Map(YsonMap<'a>),
}

// --- Construction ------------------------------------------------------------

impl<'a> YsonValue<'a> {
    /// Wraps a node with no attributes.
    #[must_use]
    pub fn new(node: YsonNode<'a>) -> Self {
        YsonValue {
            attributes: None,
            node,
        }
    }

    /// A string value, borrowing or owning whichever the argument is.
    ///
    /// ```
    /// use yson_rs::YsonValue;
    ///
    /// let borrowed = YsonValue::string(&b"host"[..]);
    /// let owned = YsonValue::string(b"host".to_vec());
    /// assert_eq!(borrowed, owned);
    /// ```
    #[must_use]
    pub fn string(bytes: impl Into<Cow<'a, [u8]>>) -> Self {
        YsonValue::new(YsonNode::String(bytes.into()))
    }
}

impl<'a> YsonNode<'a> {
    /// A string node, borrowing or owning whichever the argument is.
    #[must_use]
    pub fn string(bytes: impl Into<Cow<'a, [u8]>>) -> Self {
        YsonNode::String(bytes.into())
    }
}

// --- Reading -----------------------------------------------------------------

impl<'a> YsonValue<'a> {
    /// The node's bytes, if it is a string.
    ///
    /// Never fails on non-UTF-8, which is what YSON strings often are.
    /// Prefer it to [`YsonValue::as_str`].
    #[must_use]
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match &self.node {
            YsonNode::String(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// The node as a UTF-8 string, if it is a string and the bytes are UTF-8.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        std::str::from_utf8(self.as_bytes()?).ok()
    }

    /// The node as a signed 64-bit integer, if it is one.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        match self.node {
            YsonNode::Int64(v) => Some(v),
            _ => None,
        }
    }

    /// The node as an unsigned 64-bit integer, if it is one.
    #[must_use]
    pub fn as_u64(&self) -> Option<u64> {
        match self.node {
            YsonNode::Uint64(v) => Some(v),
            _ => None,
        }
    }

    /// The node as a double, if it is one.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self.node {
            YsonNode::Double(v) => Some(v),
            _ => None,
        }
    }

    /// The node as a boolean, if it is one.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self.node {
            YsonNode::Boolean(v) => Some(v),
            _ => None,
        }
    }

    /// The node's entries, if it is a map.
    #[must_use]
    pub fn as_map(&self) -> Option<&YsonMap<'a>> {
        match &self.node {
            YsonNode::Map(entries) => Some(entries),
            _ => None,
        }
    }

    /// Looks up a map entry by its UTF-8 name.
    ///
    /// ```
    /// use yson_rs::{Reader, YsonFormat};
    ///
    /// let value = Reader::new(b"{host=a;port=80}", YsonFormat::Text)
    ///     .read_value()
    ///     .unwrap();
    ///
    /// assert_eq!(value.get("port").unwrap().as_i64(), Some(80));
    /// assert!(value.get("absent").is_none());
    /// ```
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&YsonValue<'a>> {
        self.get_bytes(key.as_bytes())
    }

    /// Looks up a map entry by its raw byte name.
    #[must_use]
    pub fn get_bytes(&self, key: &[u8]) -> Option<&YsonValue<'a>> {
        self.as_map()?.get(key)
    }

    /// Looks up an attribute by its UTF-8 name.
    ///
    /// ```
    /// use yson_rs::{Reader, YsonFormat};
    ///
    /// let value = Reader::new(b"<schema=strict>{a=1}", YsonFormat::Text)
    ///     .read_value()
    ///     .unwrap();
    ///
    /// assert_eq!(value.attr("schema").unwrap().as_str(), Some("strict"));
    /// ```
    #[must_use]
    pub fn attr(&self, key: &str) -> Option<&YsonValue<'a>> {
        self.attr_bytes(key.as_bytes())
    }

    /// Looks up an attribute by its raw byte name.
    #[must_use]
    pub fn attr_bytes(&self, key: &[u8]) -> Option<&YsonValue<'a>> {
        self.attributes.as_ref()?.get(key)
    }
}

impl<'a> std::ops::Index<&'a str> for YsonValue<'_> {
    type Output = Self;

    /// Map entries by name, and attributes by `@name`.
    ///
    /// # Panics
    ///
    /// Panics if the key is absent, or if the value is not a map. Use
    /// [`YsonValue::get`] and [`YsonValue::attr`] where absence is expected.
    ///
    /// # Examples
    ///
    /// ```
    /// use yson_rs::{Reader, YsonFormat, YsonNode, YsonValue};
    ///
    /// let mut reader = Reader::new(b"<status=\"ok\">{id=1u}", YsonFormat::Text);
    /// let value = reader.read_value().unwrap();
    ///
    /// assert_eq!(value["@status"].as_str(), Some("ok"));
    /// assert_eq!(value["id"], YsonValue::new(YsonNode::Uint64(1)));
    /// ```
    fn index(&self, key: &'a str) -> &Self::Output {
        if let Some(name) = key.strip_prefix('@') {
            return self
                .attr_bytes(name.as_bytes())
                .expect("attribute not found");
        }
        self.get_bytes(key.as_bytes())
            .expect("key not found in map, or value is not a map")
    }
}

// --- Letting go of the input -------------------------------------------------

impl YsonValue<'_> {
    /// Copies every borrowed byte, so the value no longer refers to its input.
    ///
    /// # Examples
    ///
    /// ```
    /// use yson_rs::{OwnedYsonValue, Reader, YsonFormat};
    ///
    /// let owned: OwnedYsonValue = {
    ///     let buffer = b"{host=name}".to_vec();
    ///     Reader::new(&buffer, YsonFormat::Text).read_value().unwrap().into_owned()
    /// };
    /// assert_eq!(owned["host"].as_str(), Some("name"));
    /// ```
    #[must_use]
    pub fn into_owned(self) -> OwnedYsonValue {
        YsonValue {
            attributes: self.attributes.map(into_owned_map),
            node: self.node.into_owned(),
        }
    }
}

impl YsonNode<'_> {
    /// Copies every borrowed byte. See [`YsonValue::into_owned`].
    #[must_use]
    pub fn into_owned(self) -> YsonNode<'static> {
        match self {
            YsonNode::Entity => YsonNode::Entity,
            YsonNode::Boolean(v) => YsonNode::Boolean(v),
            YsonNode::Int64(v) => YsonNode::Int64(v),
            YsonNode::Uint64(v) => YsonNode::Uint64(v),
            YsonNode::Double(v) => YsonNode::Double(v),
            YsonNode::String(bytes) => YsonNode::String(Cow::Owned(bytes.into_owned())),
            YsonNode::List(items) => {
                YsonNode::List(items.into_iter().map(YsonValue::into_owned).collect())
            }
            YsonNode::Map(entries) => YsonNode::Map(into_owned_map(entries)),
        }
    }
}

fn into_owned_map(entries: YsonMap<'_>) -> YsonMap<'static> {
    entries
        .into_iter()
        .map(|(key, value)| (Cow::Owned(key.into_owned()), value.into_owned()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borrowed_and_owned_bytes_compare_equal() {
        // A round trip must not depend on which side of the `Cow` a value
        // landed on, or every equality assertion becomes a test of the input's
        // escaping rather than of its meaning.
        let borrowed = YsonValue::string(&b"host"[..]);
        let owned = YsonValue::string(b"host".to_vec());
        assert_eq!(borrowed, owned);

        let mut from_borrowed = YsonMap::new();
        from_borrowed.insert(Cow::Borrowed(&b"k"[..]), YsonValue::string(&b"v"[..]));
        let mut from_owned = YsonMap::new();
        from_owned.insert(Cow::Owned(b"k".to_vec()), YsonValue::string(b"v".to_vec()));
        assert_eq!(from_borrowed, from_owned);
    }

    #[test]
    fn keys_look_up_by_slice_whichever_side_they_are_on() {
        let mut entries = YsonMap::new();
        entries.insert(
            Cow::Owned(b"owned".to_vec()),
            YsonValue::new(YsonNode::Int64(1)),
        );
        entries.insert(
            Cow::Borrowed(&b"borrowed"[..]),
            YsonValue::new(YsonNode::Int64(2)),
        );
        let value = YsonValue::new(YsonNode::Map(entries));

        assert_eq!(value.get("owned").unwrap().as_i64(), Some(1));
        assert_eq!(value.get("borrowed").unwrap().as_i64(), Some(2));
        assert_eq!(value.get("absent"), None);
    }

    #[test]
    fn into_owned_detaches_the_whole_tree() {
        let owned: OwnedYsonValue = {
            let buffer = b"borrowed".to_vec();
            let mut entries = YsonMap::new();
            entries.insert(Cow::Borrowed(&buffer[..]), YsonValue::string(&buffer[..]));
            let value = YsonValue {
                attributes: Some(YsonMap::new()),
                node: YsonNode::Map(entries),
            };
            value.into_owned()
            // `buffer` dies here; `owned` must not care.
        };

        assert_eq!(
            owned.get("borrowed").unwrap().as_bytes(),
            Some(&b"borrowed"[..])
        );
    }
}
