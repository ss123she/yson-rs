use crate::core::error::YsonError;
use crate::core::{Writer, YsonFormat};
use serde::{Serialize, ser};

/// A structure for serializing Rust types into YSON byte sequences.
///
/// This is the serde binding over [`Writer`]: it decides *which* YSON shape a
/// Rust type takes, and hands the bytes themselves to the writer.
pub struct Serializer {
    /// The buffer where the serialized YSON bytes are stored.
    pub output: Vec<u8>,
    pub(crate) is_binary: bool,
    pub(crate) is_writing_attributes: bool,
}

impl Serializer {
    /// Creates a `Serializer` writing `format`, with a pre-allocated buffer.
    ///
    /// # Examples
    ///
    /// ```
    /// use yson_rs::{Serializer, YsonFormat};
    /// use serde::Serialize;
    ///
    /// let mut ser = Serializer::new(YsonFormat::Text);
    /// 42i64.serialize(&mut ser).unwrap();
    ///
    /// assert_eq!(ser.output, b"42");
    /// ```
    #[must_use]
    pub fn new(format: YsonFormat) -> Self {
        Self::with_buffer(Vec::with_capacity(8192), format)
    }

    /// Creates a `Serializer` that appends to an existing buffer.
    ///
    /// The buffer's current contents are left alone, so several values can be
    /// written into one allocation.
    ///
    /// ```
    /// use yson_rs::{Serializer, YsonFormat};
    /// use serde::Serialize;
    ///
    /// let mut buffer = Vec::new();
    /// for value in [1, 2, 3] {
    ///     let mut ser = Serializer::with_buffer(buffer, YsonFormat::Text);
    ///     value.serialize(&mut ser).unwrap();
    ///     buffer = ser.output;
    ///     buffer.push(b';');
    /// }
    /// assert_eq!(buffer, b"1;2;3;");
    /// ```
    #[must_use]
    pub fn with_buffer(buffer: Vec<u8>, format: YsonFormat) -> Self {
        Self {
            output: buffer,
            is_binary: format.is_binary(),
            is_writing_attributes: false,
        }
    }

    /// The format this serializer writes.
    #[must_use]
    pub fn format(&self) -> YsonFormat {
        if self.is_binary {
            YsonFormat::Binary
        } else {
            YsonFormat::Text
        }
    }

    /// A writer over this serializer's buffer.
    #[inline]
    pub(crate) fn writer(&mut self) -> Writer<'_> {
        let format = if self.is_binary {
            YsonFormat::Binary
        } else {
            YsonFormat::Text
        };
        Writer::new(&mut self.output, format)
    }

    #[inline]
    fn write_entity(&mut self) {
        self.writer().write_entity();
    }

    fn write_bool(&mut self, v: bool) {
        self.writer().write_bool(v);
    }

    fn write_i64(&mut self, v: i64) {
        self.writer().write_i64(v);
    }

    fn write_u64(&mut self, v: u64) {
        self.writer().write_u64(v);
    }

    fn write_f64(&mut self, v: f64) {
        self.writer().write_f64(v);
    }

    fn write_string(&mut self, v: &str) {
        self.writer().write_string(v.as_bytes());
    }
}

macro_rules! impl_serialize {
    // Numbers
    ($($name:ident($ty:ty) => $method:ident as $cast:ty),*) => {
        $(fn $name(self, v: $ty) -> Result<(), Self::Error> { self.$method(v as $cast); Ok(()) })*
    };
    // None, Unit
    (@empty $($name:ident $(($($arg:ident: $ty:ty),*))?),*) => {
        $(fn $name(self $(, $($arg: $ty),*)?) -> Result<(), Self::Error> { self.write_entity(); Ok(()) })*
    };
}

impl<'a> ser::Serializer for &'a mut Serializer {
    type Ok = ();
    type Error = YsonError;
    type SerializeSeq = Compound<'a>;
    type SerializeTuple = Compound<'a>;
    type SerializeTupleStruct = Compound<'a>;
    type SerializeTupleVariant = Compound<'a>;
    type SerializeMap = Compound<'a>;
    type SerializeStruct = Compound<'a>;
    type SerializeStructVariant = Compound<'a>;

    impl_serialize! {
        serialize_i8(i8) => write_i64 as i64, serialize_i16(i16) => write_i64 as i64,
        serialize_i32(i32) => write_i64 as i64, serialize_i64(i64) => write_i64 as i64,
        serialize_u8(u8) => write_u64 as u64, serialize_u16(u16) => write_u64 as u64,
        serialize_u32(u32) => write_u64 as u64, serialize_u64(u64) => write_u64 as u64,
        serialize_f32(f32) => write_f64 as f64, serialize_f64(f64) => write_f64 as f64
    }

    impl_serialize!(@empty serialize_none, serialize_unit, serialize_unit_struct(_n: &'static str));

    fn serialize_bool(self, v: bool) -> Result<(), Self::Error> {
        self.write_bool(v);
        Ok(())
    }
    fn serialize_char(self, v: char) -> Result<(), Self::Error> {
        self.write_string(&v.to_string());
        Ok(())
    }
    fn serialize_str(self, v: &str) -> Result<(), Self::Error> {
        self.write_string(v);
        Ok(())
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<(), Self::Error> {
        self.writer().write_bytes(v);
        Ok(())
    }

    fn serialize_some<T: ?Sized + Serialize>(self, v: &T) -> Result<(), Self::Error> {
        v.serialize(self)
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        v: &T,
    ) -> Result<(), Self::Error> {
        v.serialize(self)
    }

    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> Result<(), Self::Error> {
        self.write_string(variant);
        Ok(())
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        var: &'static str,
        val: &T,
    ) -> Result<(), Self::Error> {
        self.writer().begin_map();
        self.write_string(var);
        self.writer().key_value_separator();
        val.serialize(&mut *self)?;
        self.writer().end_map();
        Ok(())
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        self.writer().begin_list();
        Ok(Compound {
            ser: self,
            first: true,
            mode: CompoundMode::Seq,
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        var: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        self.writer().begin_map();
        self.write_string(var);
        let mut w = self.writer();
        w.key_value_separator();
        w.begin_list();
        Ok(Compound {
            ser: self,
            first: true,
            mode: CompoundMode::VariantSeq,
        })
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        let mode = if self.is_writing_attributes {
            self.writer().begin_attributes();
            CompoundMode::Attr
        } else {
            self.writer().begin_map();
            CompoundMode::Map
        };
        self.is_writing_attributes = false;
        Ok(Compound {
            ser: self,
            first: true,
            mode,
        })
    }

    fn serialize_struct(
        self,
        name: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        let mode = if name == "$__yson_attributes" {
            CompoundMode::AttrWrapper
        } else if self.is_writing_attributes {
            self.writer().begin_attributes();
            self.is_writing_attributes = false;
            CompoundMode::Attr
        } else {
            CompoundMode::Struct(StructState::Start)
        };
        Ok(Compound {
            ser: self,
            first: true,
            mode,
        })
    }

    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        var: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        self.writer().begin_map();
        self.write_string(var);
        let mut w = self.writer();
        w.key_value_separator();
        w.begin_map();
        Ok(Compound {
            ser: self,
            first: true,
            mode: CompoundMode::VariantMap,
        })
    }
}

/// How far a struct has got through the one shape a YSON document allows.
///
/// A YSON value is optional `<attributes>`, strictly before what they decorate,
/// then exactly one body. Struct fields arrive in declaration order, so they
/// have to arrive in that order too; the transitions this enum permits are the
/// whole rule.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StructState {
    /// Nothing written yet.
    Start,
    /// Inside `<`…, attributes so far.
    Attrs,
    /// Inside `{`…, a map body.
    Body,
    /// A `$value` body has been written; the value is complete.
    Value,
}

#[derive(Clone, Copy)]
enum CompoundMode {
    Seq,
    Map,
    Attr,
    AttrWrapper,
    VariantSeq,
    VariantMap,
    Struct(StructState),
}

/// A helper for serializing compound YSON types such as lists, maps, and structs.
pub struct Compound<'a> {
    ser: &'a mut Serializer,
    first: bool,
    mode: CompoundMode,
}

impl Compound<'_> {
    #[inline]
    fn check_first(&mut self) {
        if !self.first {
            self.ser.writer().item_separator();
        }
        self.first = false;
    }

    /// Writes the key of one struct field and returns the state that follows.
    ///
    /// The caller writes the value. This decides which brackets the key needs
    /// in front of it, and which orders are legal at all.
    fn write_struct_field(
        &mut self,
        state: StructState,
        key: &'static str,
    ) -> Result<StructState, YsonError> {
        if let Some(attr_name) = key.strip_prefix('@') {
            match state {
                StructState::Start => {
                    self.ser.writer().begin_attributes();
                    self.first = true;
                }
                StructState::Attrs => {}
                // Attributes stand strictly before the value they decorate.
                StructState::Body | StructState::Value => {
                    return Err(YsonError::Custom(format!(
                        "the attribute field `{key}` comes after the value body; \
                         YSON attributes stand before the value they decorate, \
                         so `@`-renamed fields must be declared first"
                    )));
                }
            }
            self.check_first();
            self.ser.write_string(attr_name);
            self.ser.writer().key_value_separator();
            return Ok(StructState::Attrs);
        }

        if key == "$value" {
            match state {
                StructState::Start => {}
                StructState::Attrs => self.ser.writer().end_attributes(),
                // One value cannot have two bodies.
                StructState::Body => {
                    return Err(YsonError::Custom(
                        "the `$value` field comes after plain fields; a value has \
                         either a `$value` body or a map body of plain fields, not both"
                            .into(),
                    ));
                }
                StructState::Value => {
                    return Err(YsonError::Custom(
                        "two `$value` fields in one struct".into(),
                    ));
                }
            }
            return Ok(StructState::Value);
        }

        match state {
            StructState::Start => {
                self.ser.writer().begin_map();
                self.first = true;
            }
            StructState::Attrs => {
                self.ser.writer().end_attributes();
                self.ser.writer().begin_map();
                self.first = true;
            }
            StructState::Body => {}
            StructState::Value => {
                return Err(YsonError::Custom(format!(
                    "the plain field `{key}` comes after a `$value` body; a value has \
                     either a `$value` body or a map body of plain fields, not both"
                )));
            }
        }
        self.check_first();
        self.ser.write_string(key);
        self.ser.writer().key_value_separator();
        Ok(StructState::Body)
    }
}

macro_rules! delegate_seq {
    ($($trait:ident),*) => {
        $(impl<'a> ser::$trait for Compound<'a> {
            type Ok = (); type Error = YsonError;
            fn serialize_element<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<(), Self::Error> {
                self.check_first(); v.serialize(&mut *self.ser)
            }
            fn end(self) -> Result<(), Self::Error> { self.ser.writer().end_list(); Ok(()) }
        })*
    };
}
delegate_seq!(SerializeSeq, SerializeTuple);

impl ser::SerializeTupleStruct for Compound<'_> {
    type Ok = ();
    type Error = YsonError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<(), Self::Error> {
        self.check_first();
        v.serialize(&mut *self.ser)
    }
    fn end(self) -> Result<(), Self::Error> {
        self.ser.writer().end_list();
        Ok(())
    }
}

impl ser::SerializeTupleVariant for Compound<'_> {
    type Ok = ();
    type Error = YsonError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<(), Self::Error> {
        self.check_first();
        v.serialize(&mut *self.ser)
    }
    fn end(self) -> Result<(), Self::Error> {
        let mut w = self.ser.writer();
        w.end_list();
        w.end_map();
        Ok(())
    }
}

impl ser::SerializeMap for Compound<'_> {
    type Ok = ();
    type Error = YsonError;
    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Self::Error> {
        self.check_first();
        key.serialize(&mut *self.ser)
    }
    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        self.ser.writer().key_value_separator();
        value.serialize(&mut *self.ser)
    }
    fn end(self) -> Result<(), Self::Error> {
        let mut w = self.ser.writer();
        if matches!(self.mode, CompoundMode::Attr) {
            w.end_attributes();
        } else {
            w.end_map();
        }
        Ok(())
    }
}

impl ser::SerializeStruct for Compound<'_> {
    type Ok = ();
    type Error = YsonError;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        match self.mode {
            CompoundMode::AttrWrapper => {
                if key == "$attributes" {
                    self.ser.is_writing_attributes = true;
                    value.serialize(&mut *self.ser)?;
                } else if key == "$value" {
                    value.serialize(&mut *self.ser)?;
                }
            }
            CompoundMode::Struct(state) => {
                let next = self.write_struct_field(state, key)?;
                self.mode = CompoundMode::Struct(next);
                value.serialize(&mut *self.ser)?;
            }
            _ => {
                self.check_first();
                self.ser.write_string(key);
                self.ser.writer().key_value_separator();
                value.serialize(&mut *self.ser)?;
            }
        }
        Ok(())
    }

    fn end(self) -> Result<(), Self::Error> {
        let mode = self.mode;
        let mut w = self.ser.writer();
        match mode {
            CompoundMode::Attr => w.end_attributes(),
            CompoundMode::Seq | CompoundMode::VariantSeq => w.end_list(),
            // Every arm leaves exactly one value behind.
            CompoundMode::Struct(state) => match state {
                StructState::Start => {
                    w.begin_map();
                    w.end_map();
                }
                StructState::Attrs => {
                    w.end_attributes();
                    w.write_entity();
                }
                StructState::Body => w.end_map(),
                StructState::Value => {}
            },
            CompoundMode::AttrWrapper => {}
            _ => w.end_map(),
        }
        Ok(())
    }
}

impl ser::SerializeStructVariant for Compound<'_> {
    type Ok = ();
    type Error = YsonError;
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        k: &'static str,
        v: &T,
    ) -> Result<(), Self::Error> {
        ser::SerializeStruct::serialize_field(self, k, v)
    }
    fn end(self) -> Result<(), Self::Error> {
        let mut w = self.ser.writer();
        w.end_map();
        w.end_map();
        Ok(())
    }
}
