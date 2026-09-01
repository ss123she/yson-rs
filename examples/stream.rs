//! Frame a list fragment one record at a time, decoding nothing - `cargo run --example stream`.

use std::io::Cursor;

use yson_rs::{FrameReader, Frames, Reader, YsonFormat};

fn main() -> Result<(), yson_rs::YsonError> {
    // A list fragment: `value; value; value`, the shape a job reads.
    let fragment = b"{host=a;port=1};{host=b;port=2};{host=c;port=3}";

    // In memory, Frames is an iterator and each frame is a slice of the input.
    for (i, frame) in Frames::new(fragment, YsonFormat::Text).enumerate() {
        let row = Reader::new(frame?, YsonFormat::Text).read_value()?;
        println!(
            "row {}: host={} port={}",
            i,
            row["host"].as_str().unwrap(),
            row["port"].as_i64().unwrap(),
        );
    }

    // Off a pipe, FrameReader reads one record at a time - forwarding its bytes reproduces the row.
    let mut frames = FrameReader::new(Cursor::new(fragment), YsonFormat::Text);
    let mut forwarded = 0;
    while let Some(frame) = frames.next_frame()? {
        forwarded += frame.len();
    }
    println!("forwarded {forwarded} bytes without decoding a single one");
    assert_eq!(forwarded, fragment.len() - 2); // the two `;` between the rows

    Ok(())
}
