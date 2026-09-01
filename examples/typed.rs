//! A Rust type in and out, through serde - `cargo run --example typed`.

use serde::{Deserialize, Serialize};
use yson_rs::{YsonFormat, from_slice, to_string, to_vec};

/// A row of the shape a YTsaurus table holds.
#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct Row {
    host: String,
    port: u16,
    online: bool,
    tags: Vec<String>,
}

fn main() -> Result<(), yson_rs::YsonError> {
    let row = Row {
        host: "example.org".to_string(),
        port: 80,
        online: true,
        tags: vec!["edge".to_string(), "cache".to_string()],
    };

    // Text is what the HTTP API speaks; binary is what a job reads and writes.
    let text = to_string(&row, YsonFormat::Text)?;
    let binary = to_vec(&row, YsonFormat::Binary)?;

    println!("text:   {text}");
    println!("binary: {:02x?}", binary);

    // Both spellings are the same value, and both decode into the same type.
    let from_text: Row = from_slice(text.as_bytes(), YsonFormat::Text)?;
    let from_binary: Row = from_slice(&binary, YsonFormat::Binary)?;
    assert_eq!(from_text, row);
    assert_eq!(from_binary, row);

    println!("decoded either way: {from_text:?}");
    Ok(())
}
