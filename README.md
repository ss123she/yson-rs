A fast and compliant [YSON](https://ytsaurus.tech/docs/en/user-guide/storage/yson) serializer and deserializer for Rust, built on top of serde.

## Features
YSON Support: Handles Text, Binary formats.

## Installation
Add this to your `Cargo.toml`:
```toml
[dependencies]
yson-rs = "0.1.3"
serde = { version = "1.0", features = ["derive"] }
```

## Performance

High-performance processing is a core goal of `yson-rs`. Benchmarks were performed on a representative dataset (~1.2MB):

| Format | Operation | Throughput | Latency |
|:--- |:--- |:--- |:--- |
| **Binary** | Serialization | **1.71 GiB/s** | 680 µs |
| | Deserialization | **255 MiB/s** | 4.66 ms |
| **Text** | Serialization | **339 MiB/s** | 2.52 ms |
| | Deserialization | **129 MiB/s** | 6.63 ms |

> [!NOTE]
> *Benchmarks performed on Intel® Core™ i5-11400 Results may vary depending on data complexity and nesting depth.*

## Working with Structs

`yson-rs` follows the conventions used in other YTsaurus libraries for mapping Rust structs to YSON.
## Working with Structs

### 1. Basic Mapping
By default, a Rust struct maps to a YSON map.

```rust
#[derive(Serialize, Deserialize)]
struct User {
    name: String,
    age: i32,
}
// YSON: {name=Alice; age=42}
```

### 2. Attributes
To treat a field as a YSON attribute, prefix its name with `@` using Serde's `#[serde(rename = "...")]`.

```rust
#[derive(Serialize, Deserialize)]
struct Table {
    #[serde(rename = "@row_count")]
    row_count: u64,
    
    path: String,
}
// YSON: <row_count=100>{path="/home/tables"}
```

### 3. Attributed Values ($value)
If you need to attach attributes to a primitive type (like a string or list), use a struct with a `$value` field.

```rust
#[derive(Serialize, Deserialize)]
struct AnnotatedString {
    #[serde(rename = "@author")]
    author: String,
    
    #[serde(rename = "$value")]
    content: String,
}
// YSON: <author=admin>"Hello world"
```

### Full Usage Example
```rust
use yson_rs::{YsonFormat, to_string, from_slice};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Table {
    #[serde(rename = "@row_count")]
    rows: u64,
    
    #[serde(rename = "@author")]
    author: String,

    #[serde(rename = "$value")]
    data: Vec<String>,
}

fn main() -> Result<(), yson_rs::YsonError> {
    let table = Table {
        rows: 2,
        author: "admin".into(),
        data: vec!["first".into(), "second".into()],
    };

    // Serialize to Text YSON
    let text_yson = to_string(&table, YsonFormat::Text)?;
    println!("Text: {}", text_yson);
    // Output: <author=admin;row_count=2u>["first";"second"]

    // Deserialize back
    let decoded: Table = from_slice(text_yson.as_bytes(), YsonFormat::Text)?;
    assert_eq!(table, decoded);

    Ok(())
}
```

### License
Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) license at your option.
