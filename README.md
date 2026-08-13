A fast and compliant [YSON](https://ytsaurus.tech/docs/en/user-guide/storage/yson) serializer and deserializer for Rust, built on top of serde.

## Features
YSON Support: Handles Text, Binary formats.

## Installation
Add this to your `Cargo.toml`:
```toml
[dependencies]
yson-rs = "0.2.0"
serde = { version = "1.0", features = ["derive"] }
```

## Performance

`cargo bench`, criterion defaults, on a ~1.2 MB dataset of 10 000 records.

| Path | Binary | Text |
|:--- |:--- |:--- |
| `scan_value` — find record boundaries, build nothing | **473 MiB/s** | 188 MiB/s |
| Serialize (serde) | 844 MiB/s | 206 MiB/s |
| `Reader::read_value` — borrowed tree | 131 MiB/s | 93 MiB/s |
| Deserialize into typed structs (serde) | 177 MiB/s | 101 MiB/s |
| `read_value().into_owned()` — for comparison | 65 MiB/s | — |

The last two rows are the point of the borrowed tree: the same parse, with and
without copying every string out of the buffer it is already in. `scan_value`
allocates nothing at all, which is why it is roughly 3.6× the cost of reading.

> [!NOTE]
> Measured on an Intel® Core™ i5-11400 with **single-channel memory and no XMP**
> (~10 GB/s). The tree-building rows are memory-bound and would be materially
> faster on a dual-channel machine; `scan_value` is compute-bound and would not
> move. Re-measure before quoting these.

## Zero-copy, without serde

`YsonValue` borrows from the buffer it was read out of. Strings, map keys and
attribute names are `Cow<[u8]>` slices of the input, so reading a large string
costs nothing beyond the tree node pointing at it — reading a document is
roughly twice as fast as reading one that copies, and the difference grows with
the payload.

```rust
use yson_rs::{Reader, YsonFormat};

let input = b"<schema=strict>{host=\"a.example\"}";
let value = Reader::new(input, YsonFormat::Text).read_value().unwrap();

assert_eq!(value["@schema"].as_str(), Some("strict"));

// The bytes are the input's bytes, not a copy of them.
let host = value["host"].as_bytes().unwrap();
assert!(input.as_ptr_range().contains(&host.as_ptr()));
```

Call `.into_owned()` to detach a value from its buffer when it has to outlive
it. Two cases allocate on the way in and cannot do otherwise: a text string
carrying backslash escapes, and a value you build by hand.

`scan_value` finds where a value ends without building one at all, which is what
framing records off a stream needs:

```rust
use yson_rs::{Scan, YsonFormat, scan_value};

let buffer = b"{a=1};{b=2}";
let Scan::Complete(len) = scan_value(buffer, YsonFormat::Text).unwrap() else {
    panic!("the first value is whole");
};
assert_eq!(&buffer[..len], b"{a=1}");

// A short buffer asks for more rather than failing.
assert_eq!(scan_value(b"{a=1;b=", YsonFormat::Text).unwrap(), Scan::Incomplete);
```

Both are in the `core` layer, which builds with `default-features = false` and
pulls in no serde.

## Working with Structs

`yson-rs` follows the conventions used in other YTsaurus libraries for mapping Rust structs to YSON.

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
