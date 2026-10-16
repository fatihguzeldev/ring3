use crate::{
    memory::{AddressSpace, GuestAddress},
    process::HostError,
};

const INVALID_PARAMETER: u32 = 0xc000000d;
const NOT_IMPLEMENTED: u32 = 0xc0000002;
struct Output {
    bytes: [u8; 512],
    length: usize,
}
impl Output {
    fn put(&mut self, value: u8) {
        if self.length < self.bytes.len() {
            self.bytes[self.length] = value;
            self.length += 1;
        }
    }
    fn repeat(&mut self, value: u8, count: usize) {
        for _ in 0..count.min(512) {
            self.put(value);
        }
    }
}
fn byte(memory: &AddressSpace, address: u32, offset: usize) -> Result<u8, HostError> {
    if offset >= 4096 {
        return Err(HostError::InvalidRequest);
    }
    let address = address
        .checked_add(u32::try_from(offset).map_err(|_| HostError::InvalidRequest)?)
        .ok_or(HostError::InvalidRequest)?;
    let mut value = [0];
    memory
        .read(GuestAddress(address), &mut value)
        .map_err(HostError::Memory)?;
    Ok(value[0])
}
fn argument(memory: &AddressSpace, address: &mut u32) -> Result<u32, HostError> {
    let mut value = [0; 4];
    memory
        .read(GuestAddress(*address), &mut value)
        .map_err(HostError::Memory)?;
    *address = address.checked_add(4).ok_or(HostError::InvalidRequest)?;
    Ok(u32::from_le_bytes(value))
}
fn digits(mut value: u64, radix: u64, uppercase: bool, out: &mut [u8; 64]) -> usize {
    let alphabet = if uppercase {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    let mut length = 0;
    loop {
        out[length] = alphabet[(value % radix) as usize];
        length += 1;
        value /= radix;
        if value == 0 {
            break;
        }
    }
    length
}
pub(super) fn format(
    memory: &AddressSpace,
    prefix: u32,
    format: u32,
    mut arguments: u32,
) -> Result<Result<([u8; 512], usize), u32>, HostError> {
    let mut output = Output {
        bytes: [0; 512],
        length: 0,
    };
    for offset in 0..512 {
        let value = byte(memory, prefix, offset)?;
        if value == 0 {
            break;
        }
        output.put(value);
    }
    let mut offset = 0;
    while output.length < 512 {
        let value = byte(memory, format, offset)?;
        offset += 1;
        if value == 0 {
            break;
        }
        if value != b'%' {
            output.put(value);
            continue;
        }
        let mut next = byte(memory, format, offset)?;
        offset += 1;
        if next == b'%' {
            output.put(b'%');
            continue;
        }
        let (mut left, mut zero, mut plus, mut alternate, mut space) =
            (false, false, false, false, false);
        while matches!(next, b'-' | b'0' | b'+' | b' ' | b'#') {
            match next {
                b'-' => left = true,
                b'0' => zero = true,
                b'+' => plus = true,
                b'#' => alternate = true,
                b' ' => space = true,
                _ => {}
            }
            next = byte(memory, format, offset)?;
            offset += 1;
        }
        let mut width: usize = 0;
        if next == b'*' {
            let raw = argument(memory, &mut arguments)? as i32;
            left |= raw < 0;
            width = raw.unsigned_abs() as usize;
            next = byte(memory, format, offset)?;
            offset += 1;
        } else {
            while next.is_ascii_digit() {
                width = width
                    .saturating_mul(10)
                    .saturating_add(usize::from(next - b'0'))
                    .min(512);
                next = byte(memory, format, offset)?;
                offset += 1;
            }
        }
        let mut precision = None;
        if next == b'.' {
            next = byte(memory, format, offset)?;
            offset += 1;
            let mut size: usize = 0;
            if next == b'*' {
                let raw = argument(memory, &mut arguments)? as i32;
                if raw >= 0 {
                    precision = Some(raw as usize);
                }
                next = byte(memory, format, offset)?;
                offset += 1;
            } else {
                while next.is_ascii_digit() {
                    size = size
                        .saturating_mul(10)
                        .saturating_add(usize::from(next - b'0'))
                        .min(512);
                    next = byte(memory, format, offset)?;
                    offset += 1;
                }
                precision = Some(size);
            }
        }
        let mut long = false;
        let mut wide = false;
        let mut short = 0;
        match next {
            b'l' | b'w' => {
                wide = true;
                next = byte(memory, format, offset)?;
                offset += 1;
                if next == b'l' {
                    long = true;
                    next = byte(memory, format, offset)?;
                    offset += 1;
                }
            }
            b'h' => {
                short = 16;
                next = byte(memory, format, offset)?;
                offset += 1;
                if next == b'h' {
                    short = 8;
                    next = byte(memory, format, offset)?;
                    offset += 1;
                }
            }
            b'I' => {
                let a = byte(memory, format, offset)?;
                let b = byte(memory, format, offset + 1)?;
                if (a, b) != (b'6', b'4') && (a, b) != (b'3', b'2') {
                    return Ok(Err(INVALID_PARAMETER));
                }
                long = a == b'6';
                offset += 2;
                next = byte(memory, format, offset)?;
                offset += 1;
            }
            b'z' | b't' => {
                next = byte(memory, format, offset)?;
                offset += 1;
            }
            _ => {}
        }
        if matches!(next, b's' | b'S') {
            let address = argument(memory, &mut arguments)?;
            let wide = wide || next == b'S';
            let limit = precision.unwrap_or(512).min(512);
            let mut text = [0; 512];
            let mut length = 0;
            if address == 0 {
                for value in b"(null)".iter().take(limit) {
                    text[length] = *value;
                    length += 1;
                }
            } else {
                for index in 0..limit {
                    let value = if wide {
                        u16::from_le_bytes([
                            byte(memory, address, index * 2)?,
                            byte(memory, address, index * 2 + 1)?,
                        ])
                    } else {
                        u16::from(byte(memory, address, index)?)
                    };
                    if value == 0 {
                        break;
                    }
                    text[length] = u8::try_from(value).unwrap_or(b'?');
                    length += 1;
                }
            }
            if !left {
                output.repeat(b' ', width.saturating_sub(length));
            }
            for value in &text[..length] {
                output.put(*value);
            }
            if left {
                output.repeat(b' ', width.saturating_sub(length));
            }
        } else if matches!(next, b'c' | b'C') {
            let value = argument(memory, &mut arguments)?;
            if !left {
                output.repeat(b' ', width.saturating_sub(1));
            }
            output.put(u8::try_from(value).unwrap_or(b'?'));
            if left {
                output.repeat(b' ', width.saturating_sub(1));
            }
        } else if matches!(next, b'd' | b'i' | b'u' | b'x' | b'X' | b'o' | b'p') {
            let low = argument(memory, &mut arguments)?;
            let raw = if long {
                u64::from(low) | (u64::from(argument(memory, &mut arguments)?) << 32)
            } else {
                u64::from(low)
            };
            let signed = matches!(next, b'd' | b'i');
            let signed_value = if long {
                raw as i64
            } else if short == 8 {
                i64::from(raw as i8)
            } else if short == 16 {
                i64::from(raw as i16)
            } else {
                i64::from(raw as i32)
            };
            let negative = signed && signed_value < 0;
            let raw = if signed {
                signed_value.unsigned_abs()
            } else if short == 8 {
                u64::from(raw as u8)
            } else if short == 16 {
                u64::from(raw as u16)
            } else {
                raw
            };
            let radix = if matches!(next, b'x' | b'X' | b'p') {
                16
            } else if next == b'o' {
                8
            } else {
                10
            };
            let mut buffer = [0; 64];
            let mut count = digits(raw, radix, next == b'X' || next == b'p', &mut buffer);
            if precision == Some(0) && raw == 0 {
                count = 0;
            }
            let sign = if negative {
                Some(b'-')
            } else if signed && plus {
                Some(b'+')
            } else if signed && space {
                Some(b' ')
            } else {
                None
            };
            let prefix = if alternate && raw != 0 && radix == 16 {
                if next == b'X' {
                    b"0X".as_slice()
                } else {
                    b"0x".as_slice()
                }
            } else if alternate && radix == 8 {
                b"0".as_slice()
            } else {
                b"".as_slice()
            };
            let minimum = if next == b'p' {
                8
            } else {
                precision.unwrap_or(0)
            };
            let zeros = minimum.saturating_sub(count);
            let size = count + zeros + prefix.len() + usize::from(sign.is_some());
            let padding = width.saturating_sub(size);
            if !left && (!zero || precision.is_some()) {
                output.repeat(b' ', padding);
            }
            if let Some(sign) = sign {
                output.put(sign);
            }
            for value in prefix {
                output.put(*value);
            }
            if !left && zero && precision.is_none() {
                output.repeat(b'0', padding);
            }
            output.repeat(b'0', zeros);
            for index in (0..count).rev() {
                output.put(buffer[index]);
            }
            if left {
                output.repeat(b' ', padding);
            }
        } else {
            return Ok(Err(
                if matches!(
                    next,
                    b'f' | b'F' | b'e' | b'E' | b'g' | b'G' | b'a' | b'A' | b'Z'
                ) {
                    NOT_IMPLEMENTED
                } else {
                    INVALID_PARAMETER
                },
            ));
        }
        if offset > 4096 {
            return Ok(Err(INVALID_PARAMETER));
        }
    }
    Ok(Ok((output.bytes, output.length)))
}
