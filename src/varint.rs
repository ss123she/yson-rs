use crate::error::YsonError;

#[inline(always)]
pub fn read_uvarint(input: &[u8]) -> Result<(u64, usize), YsonError> {
    let mut result: u64 = 0;
    let mut shift = 0;

    for (i, &byte) in input.iter().enumerate() {
        if i >= 10 {
            return Err(YsonError::Custom("Varint too long (overflow u64)".into()));
        }

        let bits = (byte & 0x7F) as u64;
        result |= bits << shift;
        if (byte & 0x80) == 0 {
            return Ok((result, i + 1));
        }
        shift += 7;
    }

    Err(YsonError::Custom(
        "Unexpected end of input while reading varint".into(),
    ))
}

#[inline(always)]
pub fn read_varint(input: &[u8]) -> Result<(i64, usize), YsonError> {
    let (u_val, consumed) = read_uvarint(input)?;
    let val = ((u_val >> 1) as i64) ^ (-((u_val & 1) as i64));
    Ok((val, consumed))
}

#[inline(always)]
pub fn write_uvarint(mut val: u64, buf: &mut Vec<u8>) {
    while val >= 0x80 {
        buf.push((val as u8) | 0x80);
        val >>= 7;
    }
    buf.push(val as u8);
}

#[inline(always)]
pub fn write_varint(val: i64, buf: &mut Vec<u8>) {
    let zigzag = ((val << 1) ^ (val >> 63)) as u64;
    write_uvarint(zigzag, buf);
}
