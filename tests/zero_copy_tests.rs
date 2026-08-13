#![cfg(feature = "serde")]

//! What reading a document actually allocates, measured with a counting
//! global allocator rather than asserted.
//!
//! Cost should track the value's *shape* -- how many lists and maps it
//! has -- and not at all how many bytes its strings hold. Two exceptions
//! are real and are pinned here: a text string carrying backslash escapes
//! must be decoded into a buffer, and an attribute name reaching the DOM
//! through serde costs one allocation for the `@` prefix.
//!
//! `allocation-counter` counts per thread, so these numbers are unaffected
//! by the rest of the suite running alongside them.

use std::hint::black_box;

use yson_rs::{
    Reader, Scan, Writer, YsonFormat, YsonNode, YsonValue, from_slice, scan_value, to_vec,
};

/// Allocation count and total bytes for one run of `body`.
fn measure(body: impl FnOnce()) -> (u64, u64) {
    let info = allocation_counter::measure(body);
    (info.count_total, info.bytes_total)
}

/// A binary document with one string of `payload` bytes, and no escapes.
fn binary_string(payload: usize) -> Vec<u8> {
    let mut out = Vec::new();
    Writer::new(&mut out, YsonFormat::Binary).write_string(&vec![b'x'; payload]);
    out
}

/// A binary map of `entries` keys, each holding a string of `payload` bytes.
fn binary_map(entries: usize, payload: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut w = Writer::new(&mut out, YsonFormat::Binary);
    w.begin_map();
    for i in 0..entries {
        if i > 0 {
            w.item_separator();
        }
        w.write_string(format!("key{i}").as_bytes());
        w.key_value_separator();
        w.write_string(&vec![b'x'; payload]);
    }
    w.end_map();
    out
}

// --- Scanning: nothing at all ------------------------------------------------

#[test]
fn scanning_never_allocates() {
    let deep = format!("{}{}", "[".repeat(60), "]".repeat(60));
    let big_map = binary_map(200, 4_096);

    let cases: Vec<(&[u8], YsonFormat)> = vec![
        (b"42", YsonFormat::Text),
        (b"hello", YsonFormat::Text),
        (b"\"quoted string\"", YsonFormat::Text),
        (b"\"a\\nb\\x41\\101c\"", YsonFormat::Text),
        (b"<a=b>{x=[1;2;3]}", YsonFormat::Text),
        (b"// comment\n{a=\"esc\\\\aped\"}", YsonFormat::Text),
        (deep.as_bytes(), YsonFormat::Text),
        (&big_map, YsonFormat::Binary),
    ];

    for (input, format) in cases {
        // Warm up anything lazily initialised before the measured run.
        let _ = scan_value(input, format);

        let (count, bytes) = measure(|| {
            black_box(scan_value(black_box(input), format)).ok();
        });

        assert_eq!(
            (count, bytes),
            (0, 0),
            "scanning {:?} allocated",
            String::from_utf8_lossy(&input[..input.len().min(32)])
        );
    }
}

#[test]
fn scanning_a_huge_document_still_allocates_nothing() {
    let input = binary_map(10_000, 1_024);
    let _ = scan_value(&input, YsonFormat::Binary);

    let (count, bytes) = measure(|| {
        assert!(matches!(
            scan_value(black_box(&input), YsonFormat::Binary),
            Ok(Scan::Complete(_))
        ));
    });

    assert_eq!((count, bytes), (0, 0));
}

// --- Reading a value: the shape costs, the bytes do not ----------------------

#[test]
fn reading_a_scalar_allocates_nothing() {
    for (input, format) in [
        (&b"#"[..], YsonFormat::Text),
        (b"42", YsonFormat::Text),
        (b"-1", YsonFormat::Text),
        (b"42u", YsonFormat::Text),
        (b"1.5", YsonFormat::Text),
        (b"%true", YsonFormat::Text),
        (b"hello", YsonFormat::Text),
        (b"\"a quoted string\"", YsonFormat::Text),
    ] {
        let _ = Reader::new(input, format).read_value();

        let (count, bytes) = measure(|| {
            black_box(Reader::new(black_box(input), format).read_value()).ok();
        });

        assert_eq!(
            (count, bytes),
            (0, 0),
            "reading {:?} allocated",
            String::from_utf8_lossy(input)
        );
    }
}

#[test]
fn reading_a_string_costs_the_same_whatever_its_length() {
    // The whole point of the borrowed DOM: a one-megabyte string is as cheap to
    // decode as a four-byte one, because neither is copied.
    let small = binary_string(4);
    let large = binary_string(1_000_000);

    let _ = Reader::new(&small, YsonFormat::Binary).read_value();

    let small_cost = measure(|| {
        black_box(Reader::new(black_box(&small), YsonFormat::Binary).read_value()).ok();
    });
    let large_cost = measure(|| {
        black_box(Reader::new(black_box(&large), YsonFormat::Binary).read_value()).ok();
    });

    assert_eq!(small_cost, (0, 0), "a small string allocated");
    assert_eq!(large_cost, (0, 0), "a large string allocated");
}

#[test]
fn a_maps_cost_tracks_its_entry_count_not_its_payload() {
    let thin = binary_map(64, 8);
    let fat = binary_map(64, 65_536);

    let _ = Reader::new(&thin, YsonFormat::Binary).read_value();

    let thin_cost = measure(|| {
        black_box(Reader::new(black_box(&thin), YsonFormat::Binary).read_value()).ok();
    });
    let fat_cost = measure(|| {
        black_box(Reader::new(black_box(&fat), YsonFormat::Binary).read_value()).ok();
    });

    // Same number of entries, payloads 8192x apart: identical cost.
    assert_eq!(
        thin_cost, fat_cost,
        "the payload size leaked into the allocation cost"
    );

    // And what it does cost is the map's own nodes, nothing per byte.
    let (count, bytes) = thin_cost;
    assert!(count > 0, "a 64-entry map must allocate its spine");
    assert!(
        bytes < 64 * 1_024,
        "a 64-entry map allocated {bytes} bytes, which looks like copied payload"
    );
}

// --- The exceptions, stated rather than hidden -------------------------------

#[test]
fn an_escaped_text_string_is_the_one_decode_that_must_copy() {
    // `"a\nb"` means three bytes that appear nowhere in the input, so they have
    // to be built. One allocation, and only for the escaped string.
    let escaped = br#""a\nb""#;
    let plain = br#""anb""#;

    let _ = Reader::new(escaped, YsonFormat::Text).read_value();

    let (escaped_count, _) = measure(|| {
        black_box(Reader::new(black_box(&escaped[..]), YsonFormat::Text).read_value()).ok();
    });
    let (plain_count, _) = measure(|| {
        black_box(Reader::new(black_box(&plain[..]), YsonFormat::Text).read_value()).ok();
    });

    assert_eq!(escaped_count, 1, "escaped strings should cost exactly one");
    assert_eq!(plain_count, 0, "an unescaped string should cost nothing");
}

#[test]
fn into_owned_is_where_the_copying_happens() {
    // The borrow has to be real, and this is the test that proves it: detaching
    // the value from its buffer costs bytes proportional to the payload.
    let small = binary_string(16);
    let large = binary_string(100_000);

    let _ = Reader::new(&small, YsonFormat::Binary)
        .read_value()
        .map(YsonValue::into_owned);

    let (_, small_bytes) = measure(|| {
        let value = Reader::new(black_box(&small), YsonFormat::Binary)
            .read_value()
            .unwrap();
        black_box(value.into_owned());
    });
    let (_, large_bytes) = measure(|| {
        let value = Reader::new(black_box(&large), YsonFormat::Binary)
            .read_value()
            .unwrap();
        black_box(value.into_owned());
    });

    assert!(
        large_bytes >= 100_000 && small_bytes < 1_000,
        "into_owned did not copy: small={small_bytes}, large={large_bytes}"
    );
}

// --- The serde path borrows too ----------------------------------------------

#[test]
fn the_serde_path_borrows_strings_and_map_keys() {
    let input = binary_map(32, 4_096);
    let _ = from_slice::<YsonValue>(&input, YsonFormat::Binary);

    let serde_cost = measure(|| {
        black_box(from_slice::<YsonValue>(
            black_box(&input),
            YsonFormat::Binary,
        ))
        .ok();
    });
    let reader_cost = measure(|| {
        black_box(Reader::new(black_box(&input), YsonFormat::Binary).read_value()).ok();
    });

    // Not necessarily identical -- serde's map visitor builds the tree through
    // a different path -- but the payload must not appear in either.
    let (_, serde_bytes) = serde_cost;
    let (_, reader_bytes) = reader_cost;
    assert!(
        serde_bytes < 32 * 1_024,
        "the serde path copied payload: {serde_bytes} bytes for a 128 KiB document"
    );
    assert!(reader_bytes < 32 * 1_024);
}

#[test]
fn a_serde_read_value_actually_points_into_the_input() {
    // Identity, not equality: the DOM's bytes must be the input's bytes.
    let input = b"{host=\"a.example\"}".to_vec();
    let value: YsonValue = from_slice(&input, YsonFormat::Text).unwrap();

    let YsonNode::Map(entries) = &value.node else {
        panic!("expected a map");
    };
    let stored = entries[b"host".as_slice()].as_bytes().unwrap();
    let key = entries.keys().next().unwrap().as_ref();

    let range = input.as_ptr_range();
    assert!(
        range.contains(&stored.as_ptr()),
        "the string was copied out of the input"
    );
    assert!(
        range.contains(&key.as_ptr()),
        "the map key was copied out of the input"
    );
}

#[test]
fn attribute_names_cost_one_allocation_each_through_serde() {
    // The flattening convention spells an attribute `@name`, and that byte has
    // to be prepended somewhere. `Reader::read_value` has no such convention
    // and stays free -- which is the reason to reach for it when it matters.
    let input = b"<a=1;b=2;c=3>{x=10}";

    let _ = from_slice::<YsonValue>(input, YsonFormat::Text);
    let _ = Reader::new(input, YsonFormat::Text).read_value();

    let (serde_count, _) = measure(|| {
        black_box(from_slice::<YsonValue>(
            black_box(&input[..]),
            YsonFormat::Text,
        ))
        .ok();
    });
    let (reader_count, _) = measure(|| {
        black_box(Reader::new(black_box(&input[..]), YsonFormat::Text).read_value()).ok();
    });

    assert!(
        serde_count >= reader_count + 3,
        "expected one allocation per attribute name: serde={serde_count}, reader={reader_count}"
    );
}

// --- Writing -----------------------------------------------------------------

#[test]
fn writing_into_a_reused_buffer_stops_allocating() {
    let value = Reader::new(b"<a=b>{x=[1;2;3];y=\"text\"}", YsonFormat::Text)
        .read_value()
        .unwrap();

    let mut buffer = Vec::with_capacity(4_096);
    Writer::new(&mut buffer, YsonFormat::Binary)
        .write_value(&value)
        .unwrap();

    // The buffer is now big enough, so writing again must not grow it.
    let (count, bytes) = measure(|| {
        buffer.clear();
        Writer::new(&mut buffer, YsonFormat::Binary)
            .write_value(black_box(&value))
            .unwrap();
    });

    assert_eq!(
        (count, bytes),
        (0, 0),
        "writing into a warm buffer allocated"
    );
}

#[test]
fn to_vec_allocates_only_its_output() {
    let value = Reader::new(b"{a=1;b=2;c=3}", YsonFormat::Text)
        .read_value()
        .unwrap();
    let _ = to_vec(&value, YsonFormat::Binary);

    let (count, _) = measure(|| {
        black_box(to_vec(black_box(&value), YsonFormat::Binary)).ok();
    });

    // One buffer, possibly grown; nothing per key or per value.
    assert!(count <= 2, "to_vec made {count} allocations for 3 entries");
}
