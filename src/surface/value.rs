//! The bridge between the DOM and serde, in both directions.
//!
//! Values serde hands over as `visit_borrowed_*` are kept as
//! `Cow::Borrowed` and never copied. `visit_str` and `visit_bytes` are
//! valid only for the duration of the call, so those two must.

use std::borrow::Cow;
use std::marker::PhantomData;

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq, SerializeStruct};
use serde::{Deserialize, Serialize, Serializer};

use crate::core::node::{YsonKey, YsonMap, YsonNode, YsonValue};

// --- Decoding ----------------------------------------------------------------

/// A map key or attribute name, kept as bytes and borrowed where it can be.
///
/// All four visitor spellings are accepted: which one arrives depends on
/// the format and on whether the bytes needed unescaping.
struct MapKey<'a>(YsonKey<'a>);

// `'de: 'a` rather than `'de == 'a`, so a borrow may be shortened to fit the
// value. Pinning them together makes `#[serde(borrow)]` fields unusable.
impl<'de: 'a, 'a> Deserialize<'de> for MapKey<'a> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct MapKeyVisitor<'a>(PhantomData<fn() -> MapKey<'a>>);

        impl<'de: 'a, 'a> Visitor<'de> for MapKeyVisitor<'a> {
            type Value = MapKey<'a>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a YSON map key or attribute name")
            }

            fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Self::Value, E> {
                Ok(MapKey(Cow::Borrowed(v.as_bytes())))
            }

            fn visit_borrowed_bytes<E: de::Error>(self, v: &'de [u8]) -> Result<Self::Value, E> {
                Ok(MapKey(Cow::Borrowed(v)))
            }

            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(MapKey(Cow::Owned(v.into_bytes())))
            }

            fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<Self::Value, E> {
                Ok(MapKey(Cow::Owned(v)))
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(MapKey(Cow::Owned(v.as_bytes().to_vec())))
            }

            fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
                Ok(MapKey(Cow::Owned(v.to_vec())))
            }
        }

        deserializer.deserialize_any(MapKeyVisitor(PhantomData))
    }
}

/// Drops the leading `@` from an attribute key without allocating: a borrowed
/// key re-borrows one byte later, an owned one shifts in place.
fn strip_at(key: YsonKey<'_>) -> YsonKey<'_> {
    match key {
        Cow::Borrowed(bytes) => Cow::Borrowed(&bytes[1..]),
        Cow::Owned(mut bytes) => {
            bytes.remove(0);
            Cow::Owned(bytes)
        }
    }
}

macro_rules! impl_visit_primitives {
    ( $( $method:ident ( $v_type:ty ) => $node_variant:ident ),* ) => {
        $(
            fn $method<E>(self, v: $v_type) -> Result<Self::Value, E> {
                Ok(YsonValue::new(YsonNode::$node_variant(v)))
            }
        )*
    };
}

impl<'de: 'a, 'a> Deserialize<'de> for YsonValue<'a> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct YsonValueVisitor<'a>(PhantomData<fn() -> YsonValue<'a>>);

        impl<'de: 'a, 'a> Visitor<'de> for YsonValueVisitor<'a> {
            type Value = YsonValue<'a>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("any YSON value")
            }

            impl_visit_primitives! {
                visit_bool(bool) => Boolean,
                visit_i64(i64) => Int64,
                visit_u64(u64) => Uint64,
                visit_f64(f64) => Double
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(YsonValue::new(YsonNode::Entity))
            }

            fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Self::Value, E> {
                Ok(YsonValue::string(v.as_bytes()))
            }

            fn visit_borrowed_bytes<E: de::Error>(self, v: &'de [u8]) -> Result<Self::Value, E> {
                Ok(YsonValue::string(v))
            }

            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(YsonValue::string(v.into_bytes()))
            }

            fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<Self::Value, E> {
                Ok(YsonValue::string(v))
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(YsonValue::string(v.as_bytes().to_vec()))
            }

            fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
                Ok(YsonValue::string(v.to_vec()))
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut items = Vec::with_capacity(seq.size_hint().unwrap_or(0));
                while let Some(item) = seq.next_element()? {
                    items.push(item);
                }
                Ok(YsonValue::new(YsonNode::List(items)))
            }

            /// An attributed value arrives flattened: `@`-keys for the
            /// attributes, and the body either as a `"$value"` entry (a
            /// scalar) or as the map's own entries at the same level. Three
            /// independent facts come out of that stream -- attributes
            /// present, `$value` present, plain keys present -- and every
            /// combination has to mean something.
            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut attributes = YsonMap::new();
                let mut plain_map = YsonMap::new();
                let mut body_node = None;

                while let Some(MapKey(key)) = map.next_key::<MapKey<'a>>()? {
                    if key.starts_with(b"@") {
                        attributes.insert(strip_at(key), map.next_value()?);
                    } else if key.as_ref() == b"$value" {
                        if body_node.is_some() {
                            return Err(de::Error::custom(
                                "an attributed value has two `$value` bodies",
                            ));
                        }
                        let value: YsonValue<'a> = map.next_value()?;
                        body_node = Some(value.node);
                        if let Some(inner) = value.attributes {
                            attributes.extend(inner);
                        }
                    } else {
                        plain_map.insert(key, map.next_value()?);
                    }
                }

                // One value cannot have two bodies.
                if body_node.is_some() && !plain_map.is_empty() {
                    let extra = plain_map.keys().next().expect("checked non-empty");
                    return Err(de::Error::custom(format_args!(
                        "an attributed value has both a `$value` body and the plain key `{}`",
                        String::from_utf8_lossy(extra)
                    )));
                }

                let node = match body_node {
                    // A scalar body: `<a=b>42`.
                    Some(node) => node,
                    // A map body beside the attributes, or a plain map.
                    None if !plain_map.is_empty() || attributes.is_empty() => {
                        YsonNode::Map(plain_map)
                    }
                    // Attributes and nothing else: `<a=b>#`. `<a=b>{}` also lands
                    // here -- the flattening consumes an empty body without emitting
                    // a key, so the two are indistinguishable by this point.
                    None => YsonNode::Entity,
                };

                Ok(YsonValue {
                    attributes: if attributes.is_empty() {
                        None
                    } else {
                        Some(attributes)
                    },
                    node,
                })
            }
        }

        deserializer.deserialize_any(YsonValueVisitor(PhantomData))
    }
}

// --- Encoding ----------------------------------------------------------------

/// A byte string, spelled the way YSON spells it: valid UTF-8 through
/// `serialize_str` so text output can use the unquoted form, everything
/// else through `serialize_bytes`. Binary output is identical either way.
struct YsonBytes<'a>(&'a [u8]);

impl Serialize for YsonBytes<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match std::str::from_utf8(self.0) {
            Ok(text) => serializer.serialize_str(text),
            Err(_) => serializer.serialize_bytes(self.0),
        }
    }
}

/// A map whose keys are byte strings.
struct YsonEntries<'a, 'b>(&'a YsonMap<'b>);

impl Serialize for YsonEntries<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in self.0 {
            map.serialize_entry(&YsonBytes(key), value)?;
        }
        map.end()
    }
}

impl Serialize for YsonNode<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            YsonNode::Entity => serializer.serialize_unit(),
            YsonNode::Boolean(v) => serializer.serialize_bool(*v),
            YsonNode::Int64(v) => serializer.serialize_i64(*v),
            YsonNode::Uint64(v) => serializer.serialize_u64(*v),
            YsonNode::Double(v) => serializer.serialize_f64(*v),
            YsonNode::String(bytes) => YsonBytes(bytes).serialize(serializer),
            YsonNode::List(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            YsonNode::Map(entries) => YsonEntries(entries).serialize(serializer),
        }
    }
}

/// Encodes a [`YsonValue`], attributes included, through the same
/// `$__yson_attributes` marker [`crate::WithAttributes`] uses.
///
/// Maps round-trip as *values*, not byte for byte: [`YsonNode::Map`] is a
/// `BTreeMap`, so keys come back sorted rather than in input order.
impl Serialize for YsonValue<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match &self.attributes {
            None => self.node.serialize(serializer),
            Some(attributes) => {
                let mut wrapper = serializer.serialize_struct("$__yson_attributes", 2)?;
                wrapper.serialize_field("$attributes", &YsonEntries(attributes))?;
                wrapper.serialize_field("$value", &self.node)?;
                wrapper.end()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stripping_an_attribute_marker_keeps_the_borrow() {
        let borrowed = strip_at(Cow::Borrowed(&b"@name"[..]));
        assert!(matches!(borrowed, Cow::Borrowed(b"name")));

        let owned = strip_at(Cow::Owned(b"@name".to_vec()));
        assert_eq!(owned.as_ref(), b"name");
    }
}
