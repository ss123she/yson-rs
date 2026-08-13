use crate::core::error::YsonError;

/// Reads an unsigned varint, returning the value and the bytes consumed.
///
/// # Errors
///
/// Returns [`YsonError::UnexpectedEof`] if the input ends mid-varint, with the
/// offset relative to `input`, and [`YsonError::Custom`] if the payload
/// encodes more than 64 bits.
#[inline]
pub fn read_uvarint(input: &[u8]) -> Result<(u64, usize), YsonError> {
    let mut result: u64 = 0;
    let mut shift = 0;

    for (i, &byte) in input.iter().enumerate() {
        // Nine groups of seven bits fill bits 0..=62, so the tenth byte carries
        // bit 63 and nothing else. Anything above that does not fit in a u64.
        if i == 9 {
            if (byte & 0x80) != 0 {
                return Err(YsonError::Custom("Varint too long (overflow u64)".into()));
            }
            if byte > 0x01 {
                return Err(YsonError::Custom(
                    "Varint payload does not fit in u64".into(),
                ));
            }
            return Ok((result | (u64::from(byte) << 63), 10));
        }

        let bits = u64::from(byte & 0x7F);
        result |= bits << shift;
        if (byte & 0x80) == 0 {
            return Ok((result, i + 1));
        }
        shift += 7;
    }

    // A short read, not a malformed one; the offset is relative to `input`.
    Err(YsonError::UnexpectedEof(input.len()))
}

/// Reads a zigzag-encoded signed varint, returning the value and the bytes consumed.
///
/// # Errors
///
/// As [`read_uvarint`].
#[inline]
pub fn read_varint(input: &[u8]) -> Result<(i64, usize), YsonError> {
    let (u_val, consumed) = read_uvarint(input)?;
    let val = ((u_val >> 1) as i64) ^ (-((u_val & 1) as i64));
    Ok((val, consumed))
}

/// Appends an unsigned varint to `buf`.
#[inline]
pub fn write_uvarint(mut val: u64, buf: &mut Vec<u8>) {
    while val >= 0x80 {
        buf.push((val as u8) | 0x80);
        val >>= 7;
    }
    buf.push(val as u8);
}

/// Appends a zigzag-encoded signed varint to `buf`.
#[inline]
pub fn write_varint(val: i64, buf: &mut Vec<u8>) {
    let zigzag = ((val << 1) ^ (val >> 63)) as u64;
    write_uvarint(zigzag, buf);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_varint_overflow_exact() {
        let mut input = vec![0x80; 11];
        input.push(0x01);
        let res = read_uvarint(&input);
        assert!(res.is_err());
    }

    #[test]
    fn a_ten_byte_varint_that_overflows_is_an_error() {
        let mut input = vec![0xFF; 9];
        input.push(0x7F);
        assert!(read_uvarint(&input).is_err());

        // Every payload above 1 in the tenth byte is out of range.
        for last in 0x02..=0x7F {
            let mut input = vec![0xFF; 9];
            input.push(last);
            assert!(
                read_uvarint(&input).is_err(),
                "accepted tenth byte {last:#x}"
            );
        }

        // An eleventh byte is refused at the tenth, by the continuation bit.
        let mut input = vec![0xFF; 10];
        input.push(0x01);
        assert!(read_uvarint(&input).is_err());
    }

    #[test]
    fn a_ten_byte_varint_that_fits_still_decodes() {
        let mut buf = Vec::new();
        write_uvarint(u64::MAX, &mut buf);
        assert_eq!(buf.len(), 10);
        assert_eq!(read_uvarint(&buf).unwrap(), (u64::MAX, 10));

        // The largest nine-byte value, and the smallest ten-byte one.
        for value in [(1u64 << 63) - 1, 1 << 63, u64::MAX - 1] {
            let mut buf = Vec::new();
            write_uvarint(value, &mut buf);
            assert_eq!(read_uvarint(&buf).unwrap(), (value, buf.len()));
        }
    }

    #[test]
    fn every_zigzag_boundary_round_trips() {
        for value in [i64::MIN, i64::MIN + 1, -1, 0, 1, i64::MAX - 1, i64::MAX] {
            let mut buf = Vec::new();
            write_varint(value, &mut buf);
            assert_eq!(read_varint(&buf).unwrap(), (value, buf.len()));
        }
    }

    #[test]
    fn test_roundtrip_varint() {
        let mut buf = Vec::new();
        write_varint(-12345, &mut buf);
        let (val, consumed) = read_varint(&buf).unwrap();
        assert_eq!(val, -12345);
        assert_eq!(consumed, buf.len());
    }
}
