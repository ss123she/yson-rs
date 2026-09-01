//! The DOM: a YSON value as a borrowed tree, no serde - `cargo run --example dom`.

use yson_rs::{Reader, Writer, YsonFormat};

fn main() -> Result<(), yson_rs::YsonError> {
    let input = b"<schema=strict>{host=\"a.example\";port=80;online=%true}";
    let value = Reader::new(input, YsonFormat::Text).read_value()?;

    // Indexing reads entries, and attributes too - marked with `@`.
    println!("schema: {}", value["@schema"].as_str().unwrap());
    println!("host:   {}", value["host"].as_str().unwrap());
    println!("port:   {}", value["port"].as_i64().unwrap());
    println!("online: {}", value["online"].as_bool().unwrap());

    // Strings are bytes, not `str` - reading copies none of them.
    let host: &[u8] = value["host"].as_bytes().unwrap();
    assert!(input.as_ptr_range().contains(&host.as_ptr()));

    // The same tree, written back out in the binary format.
    let mut out = Vec::new();
    Writer::new(&mut out, YsonFormat::Binary).write_value(&value)?;
    let reread = Reader::new(&out, YsonFormat::Binary).read_value()?;
    assert_eq!(reread, value);
    println!("binary round trip: {} bytes", out.len());

    // into_owned detaches the tree, for a value that has to outlive its buffer.
    let owned = value.into_owned();
    println!("owned host: {}", owned["host"].as_str().unwrap());

    Ok(())
}
