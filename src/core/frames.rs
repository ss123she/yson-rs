//! Cutting a stream of values into records, without decoding them.
//!
//! [`Frames`] cuts up a slice; [`FrameReader`] pulls from anything
//! [`Read`]. Each type's own page says when to reach for it.

use std::io::{ErrorKind, Read};

use crate::core::error::YsonError;
use crate::core::format::YsonFormat;
use crate::core::scan::{Scan, scan_value_with_max_depth};

/// Default read buffer, and the steady-state memory cost of a [`FrameReader`].
pub const DEFAULT_BUFFER_BYTES: usize = 64 * 1024;

/// Default ceiling on one record, to bound the damage a corrupt length prefix
/// can do.
pub const DEFAULT_MAX_RECORD_BYTES: usize = 256 * 1024 * 1024;

/// Skips separators and insignificant bytes between records.
fn skip_separators(input: &[u8]) -> usize {
    let mut i = 0;
    while i < input.len() && (input[i] == b';' || input[i].is_ascii_whitespace()) {
        i += 1;
    }
    i
}

// --- Framing a slice ---------------------------------------------------------

/// Cuts a list fragment held in memory into one frame per value.
///
/// Each frame borrows the input, so this is an ordinary [`Iterator`] and a
/// record costs nothing until something decodes it. Use [`FrameReader`] instead
/// when the fragment comes off a pipe and does not fit in memory.
///
/// Knows nothing about the YTsaurus protocol: control records, key switches and
/// row indices are a job harness's business.
///
/// # Examples
///
/// ```
/// use yson_rs::{Frames, Reader, YsonFormat};
///
/// let mut total = 0;
/// for frame in Frames::new(b"1; 2; 3", YsonFormat::Text) {
///     let value = Reader::new(frame.unwrap(), YsonFormat::Text).read_value().unwrap();
///     total += value.as_i64().unwrap();
/// }
/// assert_eq!(total, 6);
/// ```
///
/// A frame is a slice of the input, not a copy of it:
///
/// ```
/// use yson_rs::{Frames, YsonFormat};
///
/// let input = b"{host=name};2";
/// let frame = Frames::new(input, YsonFormat::Text).next().unwrap().unwrap();
/// assert!(input.as_ptr_range().contains(&frame.as_ptr()));
/// ```
pub struct Frames<'a> {
    input: &'a [u8],
    pos: usize,
    format: YsonFormat,
    max_depth: usize,
    failed: bool,
}

impl<'a> Frames<'a> {
    /// Frames `input`, read in `format`.
    #[must_use]
    pub fn new(input: &'a [u8], format: YsonFormat) -> Self {
        Frames {
            input,
            pos: 0,
            format,
            max_depth: crate::core::DEFAULT_MAX_DEPTH,
            failed: false,
        }
    }

    /// Refuses records nested deeper than `max_depth`.
    #[must_use]
    pub fn with_max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = max_depth;
        self
    }

    /// The offset of the next unread byte.
    #[must_use]
    pub const fn position(&self) -> usize {
        self.pos
    }
}

impl<'a> Iterator for Frames<'a> {
    type Item = Result<&'a [u8], YsonError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }

        self.pos += skip_separators(&self.input[self.pos..]);
        if self.pos >= self.input.len() {
            return None;
        }

        let rest = &self.input[self.pos..];
        match scan_value_with_max_depth(rest, self.format, self.max_depth) {
            Ok(Scan::Complete(len)) => {
                let frame = &rest[..len];
                self.pos += len;
                Some(Ok(frame))
            }
            // The whole input is present, so a short read means truncation.
            Ok(Scan::Incomplete) => {
                self.failed = true;
                Some(Err(YsonError::UnexpectedEof(self.pos)))
            }
            Err(e) => {
                self.failed = true;
                Some(Err(e))
            }
        }
    }
}

// --- Framing a stream --------------------------------------------------------

/// Reads a list fragment from anything [`Read`], one record at a time.
///
/// This is the entry point for a job's input: a stream far larger than memory,
/// buffered and compacted as it goes. Use [`Frames`] instead when the whole
/// fragment is already a slice.
///
/// Frames borrow an internal buffer, so this cannot be an [`Iterator`]: the
/// borrow on one frame must end before the next read moves the bytes underneath
/// it, and `&mut self` makes the compiler enforce that.
///
/// Knows nothing about the YTsaurus protocol: control records, key switches and
/// row indices are a job harness's business.
///
/// ```no_run
/// use yson_rs::{FrameReader, YsonFormat};
///
/// let mut frames = FrameReader::new(std::io::stdin().lock(), YsonFormat::Binary);
/// while let Some(record) = frames.next_frame()? {
///     // Forwarding these bytes reproduces the row exactly; decode only
///     // the rows you inspect.
///     let _ = record.len();
/// }
/// # Ok::<(), yson_rs::YsonError>(())
/// ```
pub struct FrameReader<R> {
    input: R,
    buf: Vec<u8>,
    /// Bytes of `buf` that hold input.
    filled: usize,
    /// Read cursor into `buf`.
    pos: usize,
    /// Offset of `buf[0]` within the whole stream.
    base_offset: u64,
    format: YsonFormat,
    max_depth: usize,
    max_record_bytes: usize,
    input_done: bool,
}

impl<R: Read> FrameReader<R> {
    /// Reads frames from `input`, interpreted in `format`.
    #[must_use]
    pub fn new(input: R, format: YsonFormat) -> Self {
        FrameReader {
            input,
            buf: vec![0; DEFAULT_BUFFER_BYTES],
            filled: 0,
            pos: 0,
            base_offset: 0,
            format,
            max_depth: crate::core::DEFAULT_MAX_DEPTH,
            max_record_bytes: DEFAULT_MAX_RECORD_BYTES,
            input_done: false,
        }
    }

    /// Sets the read buffer size. Records larger than this still work; the
    /// buffer grows to fit one, up to [`FrameReader::with_max_record_bytes`].
    #[must_use]
    pub fn with_buffer_size(mut self, bytes: usize) -> Self {
        self.buf = vec![0; bytes.max(64)];
        self
    }

    /// Refuses any single record larger than `bytes`.
    ///
    /// Without a ceiling a corrupt length prefix buffers until the process
    /// dies; the limit turns that into an error naming the offset.
    #[must_use]
    pub fn with_max_record_bytes(mut self, bytes: usize) -> Self {
        self.max_record_bytes = bytes;
        self
    }

    /// Refuses records nested deeper than `max_depth`.
    #[must_use]
    pub fn with_max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = max_depth;
        self
    }

    /// The stream offset of the next unread byte.
    #[must_use]
    pub fn position(&self) -> u64 {
        self.base_offset + self.pos as u64
    }

    /// Returns the next complete record, or `None` at the end of the stream.
    ///
    /// The frame borrows the reader's buffer, so it must be dropped -- or
    /// copied -- before the next call.
    ///
    /// # Errors
    ///
    /// Returns [`YsonError`] if a record is malformed, if the stream ends part
    /// way through one, or if a record exceeds the configured ceiling.
    pub fn next_frame(&mut self) -> Result<Option<&[u8]>, YsonError> {
        let len = match self.next_frame_len()? {
            Some(len) => len,
            None => return Ok(None),
        };
        let start = self.pos;
        self.pos += len;
        Ok(Some(&self.buf[start..start + len]))
    }

    /// Finds the next record's length, refilling as needed.
    ///
    /// A length does not borrow the buffer, so this can refill freely; the
    /// borrow begins only once the caller is handed the frame.
    fn next_frame_len(&mut self) -> Result<Option<usize>, YsonError> {
        loop {
            self.pos += skip_separators(&self.buf[self.pos..self.filled]);

            if self.pos < self.filled {
                let rest = &self.buf[self.pos..self.filled];
                match scan_value_with_max_depth(rest, self.format, self.max_depth) {
                    // A value ending exactly where the buffer does may not have ended:
                    // `42` is complete and also the first half of `421`. Unless the
                    // input is exhausted, read more before believing it.
                    Ok(Scan::Complete(len)) if len == rest.len() && !self.input_done => {}
                    Ok(Scan::Complete(len)) => return Ok(Some(len)),
                    // Not enough bytes yet; fall through and read more.
                    Ok(Scan::Incomplete) => {}
                    Err(e) => return Err(e),
                }
            }

            if self.input_done {
                return if self.pos == self.filled {
                    Ok(None)
                } else {
                    Err(YsonError::UnexpectedEof(self.position() as usize))
                };
            }

            self.fill()?;
        }
    }

    /// Compacts the buffer, grows it if one record needs more room, and reads once.
    fn fill(&mut self) -> Result<(), YsonError> {
        if self.pos > 0 {
            self.buf.copy_within(self.pos..self.filled, 0);
            self.filled -= self.pos;
            self.base_offset += self.pos as u64;
            self.pos = 0;
        }

        if self.filled == self.buf.len() {
            // The record in flight does not fit. Doubling is fine; chasing a
            // corrupt length prefix into an OOM abort is not.
            if self.buf.len() >= self.max_record_bytes {
                return Err(YsonError::Custom(format!(
                    "record at offset {} exceeds the {}-byte limit",
                    self.base_offset, self.max_record_bytes
                )));
            }
            let grown = self.buf.len().saturating_mul(2).min(self.max_record_bytes);
            self.buf.resize(grown, 0);
        }

        loop {
            match self.input.read(&mut self.buf[self.filled..]) {
                Ok(0) => {
                    self.input_done = true;
                    return Ok(());
                }
                Ok(n) => {
                    self.filled += n;
                    return Ok(());
                }
                // A signal interrupted the read and nothing was consumed.
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(YsonError::Custom(format!("read failed: {e}"))),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Reader, Writer};

    fn frames_of(input: &[u8], format: YsonFormat) -> Vec<Vec<u8>> {
        Frames::new(input, format)
            .map(|f| f.unwrap().to_vec())
            .collect()
    }

    /// A `Read` that yields `chunk` bytes at a time.
    struct Trickle<'a> {
        data: &'a [u8],
        chunk: usize,
    }

    impl Read for Trickle<'_> {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            let n = self.data.len().min(self.chunk).min(out.len());
            out[..n].copy_from_slice(&self.data[..n]);
            self.data = &self.data[n..];
            Ok(n)
        }
    }

    fn fragment(values: &[&str]) -> Vec<u8> {
        values.join(";").into_bytes()
    }

    // --- Slice framing -------------------------------------------------------

    #[test]
    fn a_fragment_is_cut_into_its_values() {
        assert_eq!(
            frames_of(b"{a=1};{b=2};{c=3}", YsonFormat::Text),
            [b"{a=1}".to_vec(), b"{b=2}".to_vec(), b"{c=3}".to_vec()]
        );
    }

    #[test]
    fn separators_and_whitespace_are_not_part_of_a_frame() {
        assert_eq!(
            frames_of(b" 1 ; 2 ;; 3 ; ", YsonFormat::Text),
            [b"1".to_vec(), b"2".to_vec(), b"3".to_vec()]
        );
        assert_eq!(frames_of(b"", YsonFormat::Text), Vec::<Vec<u8>>::new());
        assert_eq!(frames_of(b" ; ; ", YsonFormat::Text), Vec::<Vec<u8>>::new());
    }

    #[test]
    fn every_frame_decodes_on_its_own() {
        let input = b"<a=1>{x=[1;2]};42;\"str\";#;[{k=1}]";
        for frame in Frames::new(input, YsonFormat::Text) {
            let frame = frame.unwrap();
            assert!(
                Reader::new(frame, YsonFormat::Text).read_value().is_ok(),
                "frame does not decode: {:?}",
                std::str::from_utf8(frame)
            );
        }
    }

    #[test]
    fn a_frame_borrows_the_input() {
        let input = b"{host=name};2";
        let frame = Frames::new(input, YsonFormat::Text)
            .next()
            .unwrap()
            .unwrap();
        assert!(input.as_ptr_range().contains(&frame.as_ptr()));
    }

    #[test]
    fn a_truncated_last_record_is_an_error_on_a_slice() {
        let mut frames = Frames::new(b"{a=1};{b=", YsonFormat::Text);
        assert_eq!(frames.next().unwrap().unwrap(), b"{a=1}");
        assert!(frames.next().unwrap().is_err());
        // And the iterator stops rather than spinning on the same error.
        assert!(frames.next().is_none());
    }

    #[test]
    fn a_malformed_record_stops_the_iterator() {
        let mut frames = Frames::new(b"1;]", YsonFormat::Text);
        assert_eq!(frames.next().unwrap().unwrap(), b"1");
        assert!(frames.next().unwrap().is_err());
        assert!(frames.next().is_none());
    }

    #[test]
    fn a_depth_limit_applies_to_framing_too() {
        let input = b"[[[[1]]]];2";
        assert!(
            Frames::new(input, YsonFormat::Text)
                .with_max_depth(16)
                .all(|f| f.is_ok())
        );
        assert!(
            Frames::new(input, YsonFormat::Text)
                .with_max_depth(2)
                .any(|f| f.is_err())
        );
    }

    // --- Stream framing ------------------------------------------------------

    fn read_all<R: Read>(mut r: FrameReader<R>) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        while let Some(frame) = r.next_frame().unwrap() {
            out.push(frame.to_vec());
        }
        out
    }

    #[test]
    fn a_stream_is_cut_into_the_same_frames_as_a_slice() {
        let input = fragment(&["{a=1}", "{b=2}", "{c=3}", "42", "#"]);
        let expected = frames_of(&input, YsonFormat::Text);

        // Every chunk size, including one byte at a time, must agree.
        for chunk in [1, 2, 3, 7, 16, 1024] {
            let reader = FrameReader::new(
                Trickle {
                    data: &input,
                    chunk,
                },
                YsonFormat::Text,
            )
            .with_buffer_size(64);
            assert_eq!(read_all(reader), expected, "chunk size {chunk}");
        }
    }

    #[test]
    fn a_scalar_at_the_buffer_edge_is_not_split() {
        for input in [
            &b"42;43"[..],
            b"421",
            b"%true;%false",
            b"hello;world",
            b"1.5;2.5",
            b"12u;13u",
        ] {
            let expected = frames_of(input, YsonFormat::Text);
            for chunk in [1, 2, 3] {
                let reader = FrameReader::new(Trickle { data: input, chunk }, YsonFormat::Text)
                    .with_buffer_size(64);
                assert_eq!(
                    read_all(reader),
                    expected,
                    "{:?} at chunk size {chunk}",
                    std::str::from_utf8(input)
                );
            }
        }
    }

    #[test]
    fn a_record_larger_than_the_buffer_grows_it() {
        let mut big = Vec::new();
        Writer::new(&mut big, YsonFormat::Binary).write_string(&vec![b'x'; 10_000]);
        let input = [big.clone(), vec![b';'], big.clone()].concat();

        let reader = FrameReader::new(
            Trickle {
                data: &input,
                chunk: 100,
            },
            YsonFormat::Binary,
        )
        .with_buffer_size(64);

        let frames = read_all(reader);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0], big);
        assert_eq!(frames[1], big);
    }

    #[test]
    fn a_record_past_the_ceiling_is_refused() {
        let mut big = Vec::new();
        Writer::new(&mut big, YsonFormat::Binary).write_string(&vec![b'x'; 100_000]);

        let mut reader = FrameReader::new(&big[..], YsonFormat::Binary)
            .with_buffer_size(64)
            .with_max_record_bytes(4096);

        let err = reader.next_frame().unwrap_err();
        assert!(
            err.to_string().contains("exceeds"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn a_stream_that_stops_mid_record_is_an_error() {
        let mut reader = FrameReader::new(&b"{a=1};{b="[..], YsonFormat::Text);
        assert_eq!(reader.next_frame().unwrap().unwrap(), b"{a=1}");
        assert!(reader.next_frame().is_err());
    }

    #[test]
    fn an_empty_stream_ends_immediately() {
        let mut reader = FrameReader::new(&b""[..], YsonFormat::Text);
        assert!(reader.next_frame().unwrap().is_none());

        let mut reader = FrameReader::new(&b" ; ; "[..], YsonFormat::Text);
        assert!(reader.next_frame().unwrap().is_none());
    }

    #[test]
    fn the_position_tracks_the_whole_stream_not_the_buffer() {
        let input = fragment(&["{a=1}", "{b=2}", "{c=3}"]);
        let mut reader = FrameReader::new(
            Trickle {
                data: &input,
                chunk: 3,
            },
            YsonFormat::Text,
        )
        .with_buffer_size(64);

        let mut seen = 0u64;
        while let Some(frame) = reader.next_frame().unwrap() {
            seen += frame.len() as u64;
            // Position is past everything handed out, plus the separators.
            assert!(reader.position() >= seen);
        }
        assert_eq!(reader.position(), input.len() as u64);
    }

    #[test]
    fn an_interrupted_read_is_retried() {
        struct Interrupts {
            data: &'static [u8],
            fail_next: bool,
        }
        impl Read for Interrupts {
            fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
                if self.fail_next {
                    self.fail_next = false;
                    return Err(std::io::Error::from(ErrorKind::Interrupted));
                }
                let n = self.data.len().min(out.len());
                out[..n].copy_from_slice(&self.data[..n]);
                self.data = &self.data[n..];
                Ok(n)
            }
        }

        let reader = FrameReader::new(
            Interrupts {
                data: b"1;2;3",
                fail_next: true,
            },
            YsonFormat::Text,
        );
        assert_eq!(read_all(reader).len(), 3);
    }

    #[test]
    fn binary_streams_frame_the_same_way() {
        let mut input = Vec::new();
        for i in 0..64i64 {
            if i > 0 {
                input.push(b';');
            }
            let mut w = Writer::new(&mut input, YsonFormat::Binary);
            w.begin_map();
            w.write_string(b"id");
            w.key_value_separator();
            w.write_i64(i);
            w.end_map();
        }

        for chunk in [1, 5, 64, 4096] {
            let reader = FrameReader::new(
                Trickle {
                    data: &input,
                    chunk,
                },
                YsonFormat::Binary,
            )
            .with_buffer_size(64);
            assert_eq!(read_all(reader).len(), 64, "chunk size {chunk}");
        }
    }
}
