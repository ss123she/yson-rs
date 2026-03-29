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
            self.output.extend_from_slice(b"#");
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
        } else {
            if v.is_nan() {
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
    }
    fn write_string(&mut self, v: &str) {
        if self.is_binary {
            self.output.push(0x01);
            crate::varint::write_varint(v.len() as i64, &mut self.output);
            self.output.extend_from_slice(v.as_bytes());
        } else {
            let bytes = v.as_bytes();
            let can_be_unquoted = !bytes.is_empty()
                && (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
                && bytes
                    .iter()
                    .all(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.');

            if can_be_unquoted {
                self.output.extend_from_slice(bytes);
            } else {
                self.output.push(b'"');
                for &byte in bytes {
                    match byte {
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
                                HEX[(byte >> 4) as usize],
                                HEX[(byte & 0x0F) as usize],
                            ]);
                        }
                        _ => self.output.push(byte),
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
        self.serialize_i64(v as i64)
    }
    fn serialize_i16(self, v: i16) -> Result<(), Self::Error> {
        self.serialize_i64(v as i64)
    }
    fn serialize_i32(self, v: i32) -> Result<(), Self::Error> {
        self.serialize_i64(v as i64)
    }
    fn serialize_i64(self, v: i64) -> Result<(), Self::Error> {
        self.write_i64(v);
        Ok(())
    }
    fn serialize_u8(self, v: u8) -> Result<(), Self::Error> {
        self.serialize_u64(v as u64)
    }
    fn serialize_u16(self, v: u16) -> Result<(), Self::Error> {
        self.serialize_u64(v as u64)
    }
    fn serialize_u32(self, v: u32) -> Result<(), Self::Error> {
        self.serialize_u64(v as u64)
    }
    fn serialize_u64(self, v: u64) -> Result<(), Self::Error> {
        self.write_u64(v);
        Ok(())
    }
    fn serialize_f32(self, v: f32) -> Result<(), Self::Error> {
        self.serialize_f64(v as f64)
    }
    fn serialize_f64(self, v: f64) -> Result<(), Self::Error> {
        self.write_f64(v);
        Ok(())
    }
    fn serialize_char(self, v: char) -> Result<(), Self::Error> {
        self.serialize_str(&v.to_string())
    }
    fn serialize_str(self, v: &str) -> Result<(), Self::Error> {
        self.write_string(v);
        Ok(())
    }
    fn serialize_bytes(self, v: &[u8]) -> Result<(), Self::Error> {
        self.write_string(std::str::from_utf8(v).unwrap_or(""));
        Ok(())
    }

    fn serialize_none(self) -> Result<(), Self::Error> {
        if self.is_writing_attributes {
            self.is_writing_attributes = false;
            return Ok(());
        }
        self.write_entity();
        Ok(())
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<(), Self::Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<(), Self::Error> {
        self.serialize_none()
    }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<(), Self::Error> {
        self.serialize_unit()
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<(), Self::Error> {
        self.serialize_str(variant)
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _vi: u32,
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

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        self.output.push(b'[');
        Ok(Compound {
            ser: self,
            first: true,
            is_attributes: false,
            is_attributes_wrapper: false,
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _vi: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        self.output.push(b'{');
        self.write_string(variant);
        self.output.push(b'=');
        self.output.push(b'[');
        Ok(Compound {
            ser: self,
            first: true,
            is_attributes: false,
            is_attributes_wrapper: false,
        })
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        let is_attr = self.is_writing_attributes;
        self.is_writing_attributes = false;

        let open = if is_attr { b'<' } else { b'{' };
        self.output.push(open);
        Ok(Compound {
            ser: self,
            first: true,
            is_attributes: is_attr,
            is_attributes_wrapper: false,
        })
    }

    fn serialize_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        if name == "$__yson_attributes" {
            return Ok(Compound {
                ser: self,
                first: true,
                is_attributes: false,
                is_attributes_wrapper: true,
            });
        }
        self.serialize_map(Some(len))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _vi: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        self.output.push(b'{');
        self.write_string(variant);
        self.output.push(b'=');
        self.output.push(b'{');
        Ok(Compound {
            ser: self,
            first: true,
            is_attributes: false,
            is_attributes_wrapper: false,
        })
    }
}

pub struct Compound<'a> {
    ser: &'a mut Serializer,
    first: bool,
    is_attributes: bool,
    is_attributes_wrapper: bool,
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
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        ser::SerializeSeq::serialize_element(self, value)
    }
    fn end(self) -> Result<(), Self::Error> {
        ser::SerializeSeq::end(self)
    }
}
impl<'a> ser::SerializeTupleStruct for Compound<'a> {
    type Ok = ();
    type Error = YsonError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        ser::SerializeSeq::serialize_element(self, value)
    }
    fn end(self) -> Result<(), Self::Error> {
        ser::SerializeSeq::end(self)
    }
}
impl<'a> ser::SerializeTupleVariant for Compound<'a> {
    type Ok = ();
    type Error = YsonError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        ser::SerializeSeq::serialize_element(self, value)
    }
    fn end(self) -> Result<(), Self::Error> {
        if self.ser.is_binary {
            self.ser.output.push(0x5D);
            self.ser.output.push(0x7D);
        } else {
            self.ser.output.extend_from_slice(b"]}");
        }
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
        let close = if self.is_attributes { b'>' } else { b'}' };
        self.ser.output.push(close);
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
        if self.is_attributes_wrapper {
            if key == "$attributes" {
                self.ser.is_writing_attributes = true;
                value.serialize(&mut *self.ser)?;
                self.ser.is_writing_attributes = false;
            } else if key == "$value" {
                value.serialize(&mut *self.ser)?;
            }
            return Ok(());
        }

        if !self.first {
            self.ser.output.push(0x3B);
        }
        self.first = false;

        self.ser.write_string(key);
        self.ser.output.push(0x3D);
        value.serialize(&mut *self.ser)
    }
    fn end(self) -> Result<(), Self::Error> {
        if self.is_attributes_wrapper {
            return Ok(());
        }

        let close = if self.is_attributes { b'>' } else { b'}' };
        self.ser.output.push(close);
        Ok(())
    }
}

impl<'a> ser::SerializeStructVariant for Compound<'a> {
    type Ok = ();
    type Error = YsonError;
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        ser::SerializeStruct::serialize_field(self, key, value)
    }
    fn end(self) -> Result<(), Self::Error> {
        if self.ser.is_binary {
            self.ser.output.push(0x7D);
            self.ser.output.push(0x7D);
        } else {
            self.ser.output.extend_from_slice(b"}}");
        }
        Ok(())
    }
}
