//! Attributes beside a value: `@name`/`$value` fields or `WithAttributes` - `cargo run --example attributes`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use yson_rs::{WithAttributes, YsonFormat, from_slice, to_string};

/// `<author=alice>"hello"`: the attributes are fields of the struct.
#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct Annotated {
    #[serde(rename = "@author")]
    author: String,
    #[serde(rename = "$value")]
    content: String,
}

fn main() -> Result<(), yson_rs::YsonError> {
    let annotated = Annotated {
        author: "alice".to_string(),
        content: "hello".to_string(),
    };

    let text = to_string(&annotated, YsonFormat::Text)?;
    println!("typed:   {text}");

    let back: Annotated = from_slice(text.as_bytes(), YsonFormat::Text)?;
    assert_eq!(back, annotated);

    // The same document, into a pair instead of a struct.
    let node: WithAttributes<String, BTreeMap<String, String>> =
        from_slice(text.as_bytes(), YsonFormat::Text)?;
    println!("paired:  {:?} + {:?}", node.attributes, node.value);

    // And back out: the wrapper serializes as attributes again.
    let again = to_string(&node, YsonFormat::Text)?;
    println!("again:   {again}");

    Ok(())
}
