use std::{borrow::Cow, collections::BTreeMap};

#[derive(Debug, Clone, PartialEq)]
pub struct YsonValue {
    pub attributes: Option<BTreeMap<Vec<u8>, YsonValue>>,
    pub node: YsonNode,
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
