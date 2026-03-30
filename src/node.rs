use std::{borrow::Cow, collections::BTreeMap};

#[derive(Debug, Clone, PartialEq)]
pub struct YsonValue {
    pub attributes: Option<BTreeMap<Vec<u8>, YsonValue>>,
    pub node: YsonNode,
}

impl YsonValue {
    pub fn as_str(&self) -> Option<&str> {
        if let YsonNode::String(bytes) = &self.node {
            std::str::from_utf8(bytes).ok()
        } else {
            None
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        if let YsonNode::Int64(v) = self.node {
            Some(v)
        } else {
            None
        }
    }

    pub fn attr(&self, key: &str) -> Option<&YsonValue> {
        self.attributes.as_ref()?.get(key.as_bytes())
    }
}

impl<'a> std::ops::Index<&'a str> for YsonValue {
    type Output = YsonValue;

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

#[derive(Debug, Clone, PartialEq)]
pub enum YsonNode {
    Entity,
    Boolean(bool),
    Int64(i64),
    Uint64(u64),
    Double(f64),
    String(Vec<u8>),
    List(Vec<YsonValue>),
    Map(BTreeMap<Vec<u8>, YsonValue>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token<'a> {
    // Structural Tokens
    BeginAttributes, // <
    EndAttributes,   // >
    BeginList,       // [
    EndList,         // ]
    BeginMap,        // {
    EndMap,          // }

    // Literals
    String(Cow<'a, [u8]>),
    Int64(i64),
    Uint64(u64),
    Double(f64),
    Boolean(bool),
    Entity, // #

    // Separators
    KeyValueSeparator, // =
    ItemSeparator,     // ;
}
