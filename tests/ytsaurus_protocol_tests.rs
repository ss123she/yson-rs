#![cfg(feature = "serde")]

//! The shapes a YTsaurus job actually sees on the wire.
//!
//! A job reads a **list fragment** — `value; value; value`, not wrapped in
//! `[...]` — where data rows are interleaved with *control records*. A control
//! record is an entity carrying attributes (`<key_switch=%true>#`); a data row
//! is a map. That distinction is the whole routing rule, and it has to hold for
//! control records this crate has never heard of, because a cluster upgrade can
//! add one.
//!
//! Golden bytes here are derived from the format definition, not captured from
//! a live cluster: they pin *this crate's* encoding and prove the two formats
//! agree with each other. Agreement with the reference implementation is what
//! `interop_tests.rs` checks, against fixtures produced by the Go library.

use yson_rs::{Frames, Reader, Scan, Writer, YsonFormat, YsonNode, YsonValue, scan_value};

/// Encodes a control record: an entity with one attribute.
fn control(name: &[u8], write_value: impl Fn(&mut Writer<'_>), format: YsonFormat) -> Vec<u8> {
    let mut out = Vec::new();
    let mut w = Writer::new(&mut out, format);
    w.begin_attributes();
    w.write_string(name);
    w.key_value_separator();
    write_value(&mut w);
    w.end_attributes();
    w.write_entity();
    out
}

/// The routing rule, as a job harness would apply it.
#[derive(Debug, PartialEq)]
enum Record {
    Row,
    Control,
}

fn classify(record: &[u8], format: YsonFormat) -> Record {
    let value = Reader::new(record, format)
        .read_value()
        .expect("record parses");
    match (&value.attributes, &value.node) {
        // An attributed entity is a control record, whatever the attribute is.
        (Some(_), YsonNode::Entity) => Record::Control,
        _ => Record::Row,
    }
}

// --- Control records ---------------------------------------------------------

#[test]
fn every_control_record_has_the_documented_text_spelling() {
    let cases: [(Vec<u8>, &[u8]); 4] = [
        (
            control(b"table_index", |w| w.write_i64(1), YsonFormat::Text),
            b"<table_index=1>#",
        ),
        (
            control(b"row_index", |w| w.write_i64(42), YsonFormat::Text),
            b"<row_index=42>#",
        ),
        (
            control(b"range_index", |w| w.write_i64(0), YsonFormat::Text),
            b"<range_index=0>#",
        ),
        (
            control(b"key_switch", |w| w.write_bool(true), YsonFormat::Text),
            b"<key_switch=%true>#",
        ),
    ];

    for (written, expected) in cases {
        assert_eq!(
            written,
            expected,
            "got {:?}",
            String::from_utf8_lossy(&written)
        );
    }
}

#[test]
fn every_control_record_has_the_documented_binary_spelling() {
    // `<table_index=1>#` : `<`, string marker + zigzag(11) + name, `=`,
    // int marker + zigzag(1), `>`, `#`.
    let expected: &[u8] = &[
        b'<', 0x01, 22, b't', b'a', b'b', b'l', b'e', b'_', b'i', b'n', b'd', b'e', b'x', b'=',
        0x02, 0x02, b'>', b'#',
    ];
    assert_eq!(
        control(b"table_index", |w| w.write_i64(1), YsonFormat::Binary),
        expected
    );

    // `<key_switch=%true>#` : boolean true is the single byte 0x05.
    let expected: &[u8] = &[
        b'<', 0x01, 20, b'k', b'e', b'y', b'_', b's', b'w', b'i', b't', b'c', b'h', b'=', 0x05,
        b'>', b'#',
    ];
    assert_eq!(
        control(b"key_switch", |w| w.write_bool(true), YsonFormat::Binary),
        expected
    );
}

#[test]
fn control_records_round_trip_in_both_formats() {
    for format in [YsonFormat::Text, YsonFormat::Binary] {
        for (name, index) in [
            (&b"table_index"[..], 3i64),
            (b"row_index", 1_000_000),
            (b"range_index", 0),
        ] {
            let bytes = control(name, |w| w.write_i64(index), format);
            let value = Reader::new(&bytes, format).read_value().unwrap();

            assert_eq!(value.node, YsonNode::Entity);
            assert_eq!(
                value
                    .attr(std::str::from_utf8(name).unwrap())
                    .unwrap()
                    .as_i64(),
                Some(index)
            );
            assert_eq!(classify(&bytes, format), Record::Control);
        }
    }
}

#[test]
fn an_unknown_control_record_is_still_a_control_record() {
    // Forward compatibility: a cluster may add control attributes this version
    // has never seen. Handing one to a job as a data row would corrupt its
    // output, so the rule keys on the *shape*, not on the attribute name.
    for format in [YsonFormat::Text, YsonFormat::Binary] {
        let bytes = control(b"some_future_control", |w| w.write_i64(7), format);
        assert_eq!(classify(&bytes, format), Record::Control);
    }
}

#[test]
fn a_data_row_is_never_mistaken_for_a_control_record() {
    for format in [YsonFormat::Text, YsonFormat::Binary] {
        let mut plain = Vec::new();
        let mut w = Writer::new(&mut plain, format);
        w.begin_map();
        w.write_string(b"key_switch");
        w.key_value_separator();
        w.write_bool(true);
        w.end_map();
        // A row that merely *contains* a control-looking key is still a row.
        assert_eq!(classify(&plain, format), Record::Row);

        // And so is an attributed map, which is what a row with attributes is.
        let mut attributed = Vec::new();
        let mut w = Writer::new(&mut attributed, format);
        w.begin_attributes();
        w.write_string(b"table_index");
        w.key_value_separator();
        w.write_i64(0);
        w.end_attributes();
        w.begin_map();
        w.write_string(b"a");
        w.key_value_separator();
        w.write_i64(1);
        w.end_map();
        assert_eq!(classify(&attributed, format), Record::Row);
    }
}

// --- A reduce input stream ---------------------------------------------------

/// Builds the stream a reduce job sees: control records interleaved with rows.
fn reduce_stream(format: YsonFormat) -> Vec<u8> {
    let mut out = Vec::new();
    let push = |bytes: Vec<u8>, out: &mut Vec<u8>| {
        if !out.is_empty() {
            out.push(b';');
        }
        out.extend_from_slice(&bytes);
    };

    push(
        control(b"table_index", |w| w.write_i64(0), format),
        &mut out,
    );
    push(control(b"row_index", |w| w.write_i64(0), format), &mut out);

    for (key, value) in [("a", 1i64), ("a", 2), ("b", 3)] {
        if key == "b" {
            push(
                control(b"key_switch", |w| w.write_bool(true), format),
                &mut out,
            );
        }
        let mut row = Vec::new();
        let mut w = Writer::new(&mut row, format);
        w.begin_map();
        w.write_string(b"key");
        w.key_value_separator();
        w.write_string(key.as_bytes());
        w.item_separator();
        w.write_string(b"value");
        w.key_value_separator();
        w.write_i64(value);
        w.end_map();
        push(row, &mut out);
    }
    out
}

#[test]
fn a_reduce_stream_frames_into_its_records() {
    for format in [YsonFormat::Text, YsonFormat::Binary] {
        let stream = reduce_stream(format);
        let records: Vec<&[u8]> = Frames::new(&stream, format)
            .collect::<Result<_, _>>()
            .unwrap();

        // 2 leading control records + 3 rows + 1 key switch.
        assert_eq!(records.len(), 6, "{format:?}");

        let kinds: Vec<Record> = records.iter().map(|r| classify(r, format)).collect();
        assert_eq!(
            kinds,
            [
                Record::Control, // table_index
                Record::Control, // row_index
                Record::Row,     // key=a value=1
                Record::Row,     // key=a value=2
                Record::Control, // key_switch
                Record::Row,     // key=b value=3
            ],
            "{format:?}"
        );

        // The rows carry what they should, and the key switch falls between the
        // last `a` and the first `b`.
        let rows: Vec<YsonValue<'_>> = records
            .iter()
            .filter(|r| classify(r, format) == Record::Row)
            .map(|r| Reader::new(r, format).read_value().unwrap())
            .collect();
        let keys: Vec<&str> = rows
            .iter()
            .map(|r| r.get("key").unwrap().as_str().unwrap())
            .collect();
        assert_eq!(keys, ["a", "a", "b"]);
    }
}

#[test]
fn a_reduce_stream_frames_the_same_way_off_a_pipe() {
    use yson_rs::FrameReader;

    for format in [YsonFormat::Text, YsonFormat::Binary] {
        let stream = reduce_stream(format);
        let expected: Vec<Vec<u8>> = Frames::new(&stream, format)
            .map(|f| f.unwrap().to_vec())
            .collect();

        let mut reader = FrameReader::new(&stream[..], format).with_buffer_size(64);
        let mut got = Vec::new();
        while let Some(frame) = reader.next_frame().unwrap() {
            got.push(frame.to_vec());
        }
        assert_eq!(got, expected, "{format:?}");
    }
}

// --- Bytes that are not text -------------------------------------------------

#[test]
fn non_utf8_strings_keys_and_attribute_names_survive_both_formats() {
    // YTsaurus string columns and attribute names are arbitrary byte strings.
    let mut original = Vec::new();
    let mut w = Writer::new(&mut original, YsonFormat::Binary);
    w.begin_attributes();
    w.write_string(&[0xFF, 0xFE]);
    w.key_value_separator();
    w.write_i64(1);
    w.end_attributes();
    w.begin_map();
    w.write_string(&[0x80, b'k']);
    w.key_value_separator();
    w.write_string(&[0xC3, 0x28]);
    w.end_map();

    let value = Reader::new(&original, YsonFormat::Binary)
        .read_value()
        .unwrap();
    assert!(value.attr_bytes(&[0xFF, 0xFE]).is_some());
    assert_eq!(
        value.get_bytes(&[0x80, b'k']).unwrap().as_bytes(),
        Some(&[0xC3, 0x28][..])
    );

    for format in [YsonFormat::Text, YsonFormat::Binary] {
        let mut bytes = Vec::new();
        Writer::new(&mut bytes, format).write_value(&value).unwrap();
        assert_eq!(
            Reader::new(&bytes, format).read_value().unwrap(),
            value,
            "{format:?} lost a non-UTF-8 name"
        );
        if !format.is_binary() {
            // Text output must stay valid UTF-8, or it cannot be logged,
            // written to a text pipe, or handed to `to_string`.
            assert!(std::str::from_utf8(&bytes).is_ok());
        }
    }
}

// --- Size and shape limits ---------------------------------------------------

#[test]
fn a_string_larger_than_64_mib_round_trips() {
    // The binary string length is a varint; 64 MiB is where a naive
    // implementation's assumptions tend to break.
    let payload = vec![b'x'; 64 * 1024 * 1024 + 7];
    let mut bytes = Vec::new();
    Writer::new(&mut bytes, YsonFormat::Binary).write_string(&payload);

    let value = Reader::new(&bytes, YsonFormat::Binary)
        .read_value()
        .unwrap();
    assert_eq!(value.as_bytes().map(<[u8]>::len), Some(payload.len()));

    // And framing measures it without decoding it.
    assert_eq!(
        scan_value(&bytes, YsonFormat::Binary).unwrap(),
        Scan::Complete(bytes.len())
    );
}

#[test]
fn a_row_of_ten_thousand_columns_round_trips() {
    for format in [YsonFormat::Text, YsonFormat::Binary] {
        let mut bytes = Vec::new();
        let mut w = Writer::new(&mut bytes, format);
        w.begin_map();
        for i in 0..10_000u32 {
            if i > 0 {
                w.item_separator();
            }
            w.write_string(format!("column_{i}").as_bytes());
            w.key_value_separator();
            w.write_i64(i64::from(i));
        }
        w.end_map();

        let value = Reader::new(&bytes, format).read_value().unwrap();
        let map = value.as_map().expect("a map");
        assert_eq!(map.len(), 10_000);
        assert_eq!(value.get("column_9999").unwrap().as_i64(), Some(9999));
    }
}

#[test]
fn deeply_nested_input_is_refused_rather_than_overflowing() {
    for format in [YsonFormat::Text, YsonFormat::Binary] {
        let deep = b"[".repeat(10_000);
        assert!(Reader::new(&deep, format).read_value().is_err());

        // A balanced one recurses on the way out too.
        let mut balanced = b"[".repeat(10_000);
        balanced.extend_from_slice(&b"]".repeat(10_000));
        assert!(Reader::new(&balanced, format).read_value().is_err());
        assert!(scan_value(&balanced, format).is_err());
    }
}

#[test]
fn malformed_records_are_errors_not_panics() {
    for format in [YsonFormat::Text, YsonFormat::Binary] {
        for input in [
            &b"]"[..],
            b">",
            b"=",
            b"{1=2}",
            b"<a=1>",
            b"{a=}",
            b"{a}",
            b"[1;",
        ] {
            let _ = Reader::new(input, format).read_value();
            let _ = scan_value(input, format);
        }
    }
}

// --- The stream is a list fragment, not a list -------------------------------

#[test]
fn a_bare_fragment_is_not_a_list_and_is_not_one_value() {
    let stream = reduce_stream(YsonFormat::Text);

    // It is not a single value: reading one leaves the rest behind.
    let mut reader = Reader::new(&stream, YsonFormat::Text);
    assert!(reader.read_value().is_ok());
    assert!(reader.position() < stream.len());

    // And it is not a list: there are no brackets around it.
    assert_ne!(stream.first(), Some(&b'['));

    // Framing is what reads it.
    assert_eq!(Frames::new(&stream, YsonFormat::Text).count(), 6);
}
