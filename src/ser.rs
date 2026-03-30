use crate::error::YsonError;
use serde::{Serialize, ser};

pub struct Serializer {
    pub output: Vec<u8>,
    pub is_binary: bool,
    pub is_writing_attributes: bool,
}

impl Serializer {
    pub fn new(is_binary: bool) -> Self {
        Self {
            output: Vec::with_capacity(8192),
            is_binary,
            is_writing_attributes: false,
        }
    }

    fn write_entity(&mut self) {
        if self.is_binary {
            self.output.push(0x23);
        } else {
            self.output.push(b'#');
        }
    }

    fn write_bool(&mut self, v: bool) {
        if self.is_binary {
            self.output.push(if v { 0x05 } else { 0x04 });
        } else {
            self.output
                .extend_from_slice(if v { b"%true" } else { b"%false" });
        }
    }

    fn write_i64(&mut self, v: i64) {
        if self.is_binary {
            self.output.push(0x02);
            crate::varint::write_varint(v, &mut self.output);
        } else {
            let mut buffer = itoa::Buffer::new();
            self.output.extend_from_slice(buffer.format(v).as_bytes());
        }
    }

    fn write_u64(&mut self, v: u64) {
        if self.is_binary {
            self.output.push(0x06);
            crate::varint::write_uvarint(v, &mut self.output);
        } else {
            let mut buffer = itoa::Buffer::new();
            self.output.extend_from_slice(buffer.format(v).as_bytes());
            self.output.push(b'u');
        }
    }

    fn write_f64(&mut self, v: f64) {
        if self.is_binary {
            self.output.push(0x03);
            self.output.extend_from_slice(&v.to_le_bytes());
        } else if v.is_nan() {
            self.output.extend_from_slice(b"%nan");
        } else if v.is_infinite() {
            if v.is_sign_negative() {
                self.output.extend_from_slice(b"%-inf");
            } else {
                self.output.extend_from_slice(b"%inf");
            }
        } else {
            let mut buffer = ryu::Buffer::new();
            let s = buffer.format(v);
            self.output.extend_from_slice(s.as_bytes());
            if !s.contains('.') && !s.contains('e') && !s.contains('E') {
                self.output.extend_from_slice(b".0");
            }
        }
    }

    fn write_string(&mut self, v: &str) {
        if self.is_binary {
            self.output.push(0x01);
            crate::varint::write_varint(v.len() as i64, &mut self.output);
            self.output.extend_from_slice(v.as_bytes());
        } else {
            let bytes = v.as_bytes();
            let safe = !bytes.is_empty()
                && (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
                && bytes
                    .iter()
                    .all(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.');

            if safe {
                self.output.extend_from_slice(bytes);
            } else {
                self.output.push(b'"');
                for &b in bytes {
                    match b {
                        b'"' => self.output.extend_from_slice(b"\\\""),
                        b'\\' => self.output.extend_from_slice(b"\\\\"),
                        b'\n' => self.output.extend_from_slice(b"\\n"),
                        b'\r' => self.output.extend_from_slice(b"\\r"),
                        b'\t' => self.output.extend_from_slice(b"\\t"),
                        0x00..=0x1F => {
                            const HEX: &[u8] = b"0123456789abcdef";
                            self.output.extend_from_slice(&[
                                b'\\',
                                b'x',
                                HEX[(b >> 4) as usize],
                                HEX[(b & 0x0F) as usize],
                            ]);
                        }
                        _ => self.output.push(b),
                    }
                }
                self.output.push(b'"');
            }
        }
    }
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

    fn serialize_bool(self, v: bool) -> Result<(), Self::Error> {
        self.write_bool(v);
        Ok(())
    }
    fn serialize_i8(self, v: i8) -> Result<(), Self::Error> {
        self.write_i64(v as i64);
        Ok(())
    }
    fn serialize_i16(self, v: i16) -> Result<(), Self::Error> {
        self.write_i64(v as i64);
        Ok(())
    }
    fn serialize_i32(self, v: i32) -> Result<(), Self::Error> {
        self.write_i64(v as i64);
        Ok(())
    }
    fn serialize_i64(self, v: i64) -> Result<(), Self::Error> {
        self.write_i64(v);
        Ok(())
    }
    fn serialize_u8(self, v: u8) -> Result<(), Self::Error> {
        self.write_u64(v as u64);
        Ok(())
    }
    fn serialize_u16(self, v: u16) -> Result<(), Self::Error> {
        self.write_u64(v as u64);
        Ok(())
    }
    fn serialize_u32(self, v: u32) -> Result<(), Self::Error> {
        self.write_u64(v as u64);
        Ok(())
    }
    fn serialize_u64(self, v: u64) -> Result<(), Self::Error> {
        self.write_u64(v);
        Ok(())
    }
    fn serialize_f32(self, v: f32) -> Result<(), Self::Error> {
        self.write_f64(v as f64);
        Ok(())
    }
    fn serialize_f64(self, v: f64) -> Result<(), Self::Error> {
        self.write_f64(v);
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
        if self.is_binary {
            self.output.push(0x01);
            crate::varint::write_varint(v.len() as i64, &mut self.output);
            self.output.extend_from_slice(v);
        } else {
            self.write_string(&String::from_utf8_lossy(v));
        }
        Ok(())
    }

    fn serialize_none(self) -> Result<(), Self::Error> {
        self.write_entity();
        Ok(())
    }
    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<(), Self::Error> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<(), Self::Error> {
        self.write_entity();
        Ok(())
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<(), Self::Error> {
        self.write_entity();
        Ok(())
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
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        self.output.push(b'{');
        self.write_string(variant);
        self.output.push(b'=');
        value.serialize(&mut *self)?;
        self.output.push(b'}');
        Ok(())
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        self.output.push(b'[');
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
        variant: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        self.output.push(b'{');
        self.write_string(variant);
        self.output.push(b'=');
        self.output.push(b'[');
        Ok(Compound {
            ser: self,
            first: true,
            mode: CompoundMode::VariantSeq,
        })
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        let open = if self.is_writing_attributes {
            b'<'
        } else {
            b'{'
        };
        self.output.push(open);
        let mode = if self.is_writing_attributes {
            CompoundMode::Attr
        } else {
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
        fields: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        if name == "$__yson_attributes" {
            return Ok(Compound {
                ser: self,
                first: true,
                mode: CompoundMode::AttrWrapper,
            });
        }

        if fields > 0 && name != "$__yson_attributes" {}

        if self.is_writing_attributes {
            self.output.push(b'<');
            self.is_writing_attributes = false;
            return Ok(Compound {
                ser: self,
                first: true,
                mode: CompoundMode::Attr,
            });
        }

        Ok(Compound {
            ser: self,
            first: true,
            mode: CompoundMode::Struct {
                attr_open: false,
                body_open: false,
            },
        })
    }

    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        self.output.push(b'{');
        self.write_string(variant);
        self.output.push(b'=');
        self.output.push(b'{');
        Ok(Compound {
            ser: self,
            first: true,
            mode: CompoundMode::VariantMap,
        })
    }
}

enum CompoundMode {
    Seq,
    Map,
    Attr,
    AttrWrapper,
    VariantSeq,
    VariantMap,
    Struct { attr_open: bool, body_open: bool },
}

pub struct Compound<'a> {
    ser: &'a mut Serializer,
    first: bool,
    mode: CompoundMode,
}

impl<'a> ser::SerializeSeq for Compound<'a> {
    type Ok = ();
    type Error = YsonError;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        if !self.first {
            self.ser.output.push(b';');
        }
        self.first = false;
        value.serialize(&mut *self.ser)
    }
    fn end(self) -> Result<(), Self::Error> {
        self.ser.output.push(b']');
        Ok(())
    }
}

impl<'a> ser::SerializeTuple for Compound<'a> {
    type Ok = ();
    type Error = YsonError;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<(), Self::Error> {
        ser::SerializeSeq::serialize_element(self, v)
    }
    fn end(self) -> Result<(), Self::Error> {
        ser::SerializeSeq::end(self)
    }
}

impl<'a> ser::SerializeTupleStruct for Compound<'a> {
    type Ok = ();
    type Error = YsonError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<(), Self::Error> {
        ser::SerializeSeq::serialize_element(self, v)
    }
    fn end(self) -> Result<(), Self::Error> {
        ser::SerializeSeq::end(self)
    }
}

impl<'a> ser::SerializeTupleVariant for Compound<'a> {
    type Ok = ();
    type Error = YsonError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<(), Self::Error> {
        ser::SerializeSeq::serialize_element(self, v)
    }
    fn end(self) -> Result<(), Self::Error> {
        self.ser.output.extend_from_slice(b"]}");
        Ok(())
    }
}

impl<'a> ser::SerializeMap for Compound<'a> {
    type Ok = ();
    type Error = YsonError;
    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Self::Error> {
        if !self.first {
            self.ser.output.push(b';');
        }
        self.first = false;
        key.serialize(&mut *self.ser)
    }
    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        self.ser.output.push(b'=');
        value.serialize(&mut *self.ser)
    }
    fn end(self) -> Result<(), Self::Error> {
        self.ser.output.push(match self.mode {
            CompoundMode::Attr => b'>',
            _ => b'}',
        });
        Ok(())
    }
}

impl<'a> ser::SerializeStruct for Compound<'a> {
    type Ok = ();
    type Error = YsonError;
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        match &mut self.mode {
            CompoundMode::AttrWrapper => {
                if key == "$attributes" {
                    self.ser.is_writing_attributes = true;
                    value.serialize(&mut *self.ser)?;
                } else if key == "$value" {
                    value.serialize(&mut *self.ser)?;
                }
            }
            CompoundMode::Attr => {
                if !self.first {
                    self.ser.output.push(b';');
                }
                self.ser.write_string(key);
                self.ser.output.push(b'=');
                value.serialize(&mut *self.ser)?;
                self.first = false;
            }
            CompoundMode::Struct {
                attr_open,
                body_open,
            } => {
                if key.starts_with('@') {
                    if !*attr_open {
                        self.ser.output.push(b'<');
                        *attr_open = true;
                        self.first = true;
                    }
                    if !self.first {
                        self.ser.output.push(b';');
                    }
                    self.ser.write_string(&key[1..]);
                    self.ser.output.push(b'=');
                    value.serialize(&mut *self.ser)?;
                    self.first = false;
                } else if key == "$value" {
                    if *attr_open {
                        self.ser.output.push(b'>');
                        *attr_open = false;
                    }
                    value.serialize(&mut *self.ser)?;
                } else {
                    if *attr_open {
                        self.ser.output.push(b'>');
                        *attr_open = false;
                    }
                    if !*body_open {
                        self.ser.output.push(b'{');
                        *body_open = true;
                        self.first = true;
                    }
                    if !self.first {
                        self.ser.output.push(b';');
                    }
                    self.ser.write_string(key);
                    self.ser.output.push(b'=');
                    value.serialize(&mut *self.ser)?;
                    self.first = false;
                }
            }
            _ => {
                if !self.first {
                    self.ser.output.push(b';');
                }
                self.ser.write_string(key);
                self.ser.output.push(b'=');
                value.serialize(&mut *self.ser)?;
                self.first = false;
            }
        }
        Ok(())
    }

    fn end(self) -> Result<(), Self::Error> {
        match self.mode {
            CompoundMode::Attr => self.ser.output.push(b'>'),
            CompoundMode::AttrWrapper => {}
            CompoundMode::Seq | CompoundMode::VariantSeq => self.ser.output.push(b']'),
            CompoundMode::Struct {
                attr_open,
                body_open,
            } => {
                if attr_open {
                    self.ser.output.push(b'>');
                }
                if body_open {
                    self.ser.output.push(b'}');
                }
                if !attr_open && !body_open {}
            }
            _ => self.ser.output.push(b'}'),
        }
        Ok(())
    }
}

impl<'a> ser::SerializeStructVariant for Compound<'a> {
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
        self.ser.output.extend_from_slice(b"}}");
        Ok(())
    }
}
