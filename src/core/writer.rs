use crate::core::DEFAULT_MAX_DEPTH;
use crate::core::error::YsonError;
use crate::core::format::YsonFormat;
use crate::core::node::{YsonNode, YsonValue};
use crate::core::varint;

/// Emits YSON bytes into a caller-owned buffer, in either format.
///
/// The writer borrows its output rather than owning it, so several values can
/// be written into one allocation. Every string method takes `&[u8]`: YSON
/// strings, map keys and attribute names are arbitrary byte strings.
///
/// # Examples
///
/// ```
/// use yson_rs::core::{Writer, YsonFormat};
///
/// let mut out = Vec::new();
/// let mut w = Writer::new(&mut out, YsonFormat::Text);
/// w.begin_map();
/// w.write_string(b"host");
/// w.key_value_separator();
/// w.write_i64(42);
/// w.end_map();
///
/// assert_eq!(out, b"{host=42}");
/// ```
pub struct Writer<'a> {
    out: &'a mut Vec<u8>,
    format: YsonFormat,
}

impl<'a> Writer<'a> {
    /// Creates a writer appending to `out` in `format`.
    #[must_use]
    pub fn new(out: &'a mut Vec<u8>, format: YsonFormat) -> Self {
        Self { out, format }
    }

    /// The format this writer was built with.
    #[must_use]
    pub const fn format(&self) -> YsonFormat {
        self.format
    }

    /// The bytes written so far, including anything the buffer already held.
    #[must_use]
    pub fn buffer(&self) -> &[u8] {
        self.out
    }

    /// Writes the entity marker, `#`.
    #[inline]
    pub fn write_entity(&mut self) {
        self.out.push(0x23);
    }

    /// Writes a boolean.
    pub fn write_bool(&mut self, v: bool) {
        if self.format.is_binary() {
            self.out.push(if v { 0x05 } else { 0x04 });
        } else {
            self.out
                .extend_from_slice(if v { b"%true" } else { b"%false" });
        }
    }

    /// Writes a signed 64-bit integer.
    pub fn write_i64(&mut self, v: i64) {
        if self.format.is_binary() {
            self.out.push(0x02);
            varint::write_varint(v, self.out);
        } else {
            self.out
                .extend_from_slice(itoa::Buffer::new().format(v).as_bytes());
        }
    }

    /// Writes an unsigned 64-bit integer, with the `u` suffix in text mode.
    pub fn write_u64(&mut self, v: u64) {
        if self.format.is_binary() {
            self.out.push(0x06);
            varint::write_uvarint(v, self.out);
        } else {
            self.out
                .extend_from_slice(itoa::Buffer::new().format(v).as_bytes());
            self.out.push(b'u');
        }
    }

    /// Writes a double, including the `%nan`, `%inf` and `%-inf` spellings.
    pub fn write_f64(&mut self, v: f64) {
        if self.format.is_binary() {
            self.out.push(0x03);
            self.out.extend_from_slice(&v.to_le_bytes());
        } else if v.is_nan() {
            self.out.extend_from_slice(b"%nan");
        } else if v.is_infinite() {
            self.out.extend_from_slice(if v.is_sign_negative() {
                b"%-inf"
            } else {
                b"%inf"
            });
        } else {
            let s = ryu::Buffer::new().format(v).to_owned();
            self.out.extend_from_slice(s.as_bytes());
            if !s.contains(&['.', 'e', 'E'][..]) {
                self.out.extend_from_slice(b".0");
            }
        }
    }

    /// Writes a string, leaving it unquoted in text mode where YSON allows it.
    ///
    /// A value goes bare when its first byte is a letter or `_` and the rest
    /// are alphanumeric or `_-.`; everything else is quoted and escaped. This
    /// is the spelling for map keys, attribute names and identifier-shaped
    /// values.
    ///
    /// Bytes that are not valid UTF-8 are hex-escaped, so text output is
    /// always valid UTF-8.
    ///
    /// # Examples
    ///
    /// ```
    /// use yson_rs::{Writer, YsonFormat};
    ///
    /// let mut out = Vec::new();
    /// let mut w = Writer::new(&mut out, YsonFormat::Text);
    /// w.write_string(b"host");
    /// w.item_separator();
    /// w.write_string(b"a b");
    /// assert_eq!(out, b"host;\"a b\"");
    /// ```
    pub fn write_string(&mut self, v: &[u8]) {
        if self.format.is_binary() {
            self.write_binary_string(v);
        } else if is_safe_unquoted(v) {
            self.out.extend_from_slice(v);
        } else {
            self.write_quoted(v, std::str::from_utf8(v).is_err());
        }
    }

    /// Writes a byte string, always quoted in text mode.
    ///
    /// Unlike [`Writer::write_string`] this escapes every byte outside
    /// printable ASCII, so the output is ASCII-safe whatever the input holds.
    pub fn write_bytes(&mut self, v: &[u8]) {
        if self.format.is_binary() {
            self.write_binary_string(v);
        } else {
            self.write_quoted(v, true);
        }
    }

    /// Writes `v` quoted, escaping what the grammar requires.
    ///
    /// With `ascii_only`, every byte outside printable ASCII is hex-escaped;
    /// otherwise bytes above 0x7E pass through, which is safe only for UTF-8.
    fn write_quoted(&mut self, v: &[u8], ascii_only: bool) {
        self.out.push(b'"');
        for &b in v {
            match b {
                b'"' => self.out.extend_from_slice(b"\\\""),
                b'\\' => self.out.extend_from_slice(b"\\\\"),
                b'\n' => self.out.extend_from_slice(b"\\n"),
                b'\r' => self.out.extend_from_slice(b"\\r"),
                b'\t' => self.out.extend_from_slice(b"\\t"),
                0x20..=0x7E => self.out.push(b),
                _ if ascii_only => self.write_hex_escape(b),
                0x00..=0x1F => self.write_hex_escape(b),
                _ => self.out.push(b),
            }
        }
        self.out.push(b'"');
    }

    fn write_binary_string(&mut self, v: &[u8]) {
        self.out.push(0x01);
        varint::write_varint(v.len() as i64, self.out);
        self.out.extend_from_slice(v);
    }

    fn write_hex_escape(&mut self, b: u8) {
        const HEX: &[u8] = b"0123456789abcdef";
        self.out.extend_from_slice(&[
            b'\\',
            b'x',
            HEX[(b >> 4) as usize],
            HEX[(b & 0x0F) as usize],
        ]);
    }

    /// Writes `[`.
    #[inline]
    pub fn begin_list(&mut self) {
        self.out.push(b'[');
    }

    /// Writes `]`.
    #[inline]
    pub fn end_list(&mut self) {
        self.out.push(b']');
    }

    /// Writes `{`.
    #[inline]
    pub fn begin_map(&mut self) {
        self.out.push(b'{');
    }

    /// Writes `}`.
    #[inline]
    pub fn end_map(&mut self) {
        self.out.push(b'}');
    }

    /// Writes `<`.
    #[inline]
    pub fn begin_attributes(&mut self) {
        self.out.push(b'<');
    }

    /// Writes `>`.
    #[inline]
    pub fn end_attributes(&mut self) {
        self.out.push(b'>');
    }

    /// Writes `=`.
    #[inline]
    pub fn key_value_separator(&mut self) {
        self.out.push(b'=');
    }

    /// Writes `;`.
    #[inline]
    pub fn item_separator(&mut self) {
        self.out.push(b';');
    }

    /// Writes a whole [`YsonValue`] tree, attributes included.
    ///
    /// Map keys are emitted in `BTreeMap` order, so this does not reproduce the
    /// key order of the bytes a tree was read from.
    ///
    /// # Examples
    ///
    /// ```
    /// use yson_rs::{Reader, Writer, YsonFormat};
    ///
    /// let value = Reader::new(b"<a=1>{b=2}", YsonFormat::Text).read_value().unwrap();
    ///
    /// let mut out = Vec::new();
    /// Writer::new(&mut out, YsonFormat::Text).write_value(&value).unwrap();
    /// assert_eq!(out, b"<a=1>{b=2}");
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`YsonError`] if the tree nests deeper than
    /// [`crate::core::DEFAULT_MAX_DEPTH`]; the buffer may hold a partial value
    /// in that case.
    pub fn write_value(&mut self, value: &YsonValue<'_>) -> Result<(), YsonError> {
        self.write_value_with_max_depth(value, DEFAULT_MAX_DEPTH)
    }

    /// Writes a [`YsonValue`], refusing a tree nested deeper than `max_depth`.
    ///
    /// # Errors
    ///
    /// As [`Writer::write_value`], with the caller's depth limit.
    pub fn write_value_with_max_depth(
        &mut self,
        value: &YsonValue<'_>,
        max_depth: usize,
    ) -> Result<(), YsonError> {
        self.write_value_at(value, 0, max_depth)
    }

    fn write_value_at(
        &mut self,
        value: &YsonValue<'_>,
        depth: usize,
        max: usize,
    ) -> Result<(), YsonError> {
        if depth > max {
            return Err(YsonError::Custom("Recursion limit exceeded".into()));
        }

        if let Some(attributes) = &value.attributes {
            self.begin_attributes();
            for (i, (key, val)) in attributes.iter().enumerate() {
                if i > 0 {
                    self.item_separator();
                }
                self.write_string(key);
                self.key_value_separator();
                self.write_value_at(val, depth + 1, max)?;
            }
            self.end_attributes();
        }
        self.write_node_at(&value.node, depth, max)
    }

    fn write_node_at(
        &mut self,
        node: &YsonNode<'_>,
        depth: usize,
        max: usize,
    ) -> Result<(), YsonError> {
        if depth > max {
            return Err(YsonError::Custom("Recursion limit exceeded".into()));
        }

        match node {
            YsonNode::Entity => self.write_entity(),
            YsonNode::Boolean(b) => self.write_bool(*b),
            YsonNode::Int64(v) => self.write_i64(*v),
            YsonNode::Uint64(v) => self.write_u64(*v),
            YsonNode::Double(v) => self.write_f64(*v),
            YsonNode::String(s) => self.write_string(s),
            YsonNode::List(items) => {
                self.begin_list();
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        self.item_separator();
                    }
                    self.write_value_at(item, depth + 1, max)?;
                }
                self.end_list();
            }
            YsonNode::Map(entries) => {
                self.begin_map();
                for (i, (key, val)) in entries.iter().enumerate() {
                    if i > 0 {
                        self.item_separator();
                    }
                    self.write_string(key);
                    self.key_value_separator();
                    self.write_value_at(val, depth + 1, max)?;
                }
                self.end_map();
            }
        }
        Ok(())
    }
}

/// Whether a text-mode string can be written without quotes: a leading letter
/// or `_`, then alphanumerics and `_-.`.
fn is_safe_unquoted(b: &[u8]) -> bool {
    matches!(b.first(), Some(f) if f.is_ascii_alphabetic() || *f == b'_')
        && b.iter()
            .all(|&c| c.is_ascii_alphanumeric() || b"_-.".contains(&c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Reader;

    #[test]
    fn a_caller_can_tighten_the_write_depth_limit() {
        let value = Reader::new(b"[[[[1]]]]", YsonFormat::Text)
            .read_value()
            .unwrap();

        let mut out = Vec::new();
        assert!(
            Writer::new(&mut out, YsonFormat::Text)
                .write_value_with_max_depth(&value, 16)
                .is_ok()
        );

        let mut out = Vec::new();
        assert!(
            Writer::new(&mut out, YsonFormat::Text)
                .write_value_with_max_depth(&value, 2)
                .is_err()
        );
    }
}
