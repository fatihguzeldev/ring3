use super::{Access, DispatchError, ERRNO, GuestMemory, MemoryError, guest};

pub(super) fn sscanf(
    memory: &mut GuestMemory,
    args: &[u32],
    stack: u32,
) -> Result<u32, DispatchError> {
    if args[0] == 0 || args[1] == 0 {
        guest::write_word(memory, ERRNO, 22)?;
        return Ok(u32::MAX);
    }
    let format = read_string(memory, args[1])?;
    if format != b"%d" && format != b"%f" && format != b"%u" && format != b"%x" {
        return Err(DispatchError::Unsupported);
    }
    let input = read_string(memory, args[0])?;
    let mut offset = 0;
    while input.get(offset).is_some_and(u8::is_ascii_whitespace) {
        offset += 1;
    }
    if offset == input.len() {
        return Ok(u32::MAX);
    }
    let value = match format.as_slice() {
        b"%d" => parse_decimal(&input, offset)?,
        b"%f" => parse_float(&input, offset)?,
        b"%u" => parse_unsigned(&input, offset)?,
        b"%x" => parse_hex(&input, offset)?,
        _ => unreachable!("validated format"),
    };
    let Some(value) = value else {
        return Ok(0);
    };
    let address = stack.checked_add(12).ok_or(MemoryError::AddressOverflow)?;
    let mut destination = [0];
    guest::read_words(memory, address, &mut destination)?;
    guest::check(memory, destination[0], 4, Access::Write)?;
    memory.write(u64::from(destination[0]), &value)?;
    Ok(1)
}

fn parse_decimal(input: &[u8], mut offset: usize) -> Result<Option<[u8; 4]>, DispatchError> {
    let negative = match input[offset] {
        b'-' => {
            offset += 1;
            true
        }
        b'+' => {
            offset += 1;
            false
        }
        _ => false,
    };
    let start = offset;
    let mut magnitude = 0_u64;
    let limit = if negative {
        2_147_483_648_u64
    } else {
        2_147_483_647_u64
    };
    while matches!(input.get(offset), Some(b'0'..=b'9')) {
        let digit = input[offset] - b'0';
        magnitude = magnitude * 10 + u64::from(digit);
        if magnitude > limit {
            return Err(DispatchError::Unsupported);
        }
        offset += 1;
    }
    if offset == start {
        return Ok(None);
    }
    let signed = i64::try_from(magnitude).expect("bounded decimal magnitude");
    let value = i32::try_from(if negative { -signed } else { signed })
        .expect("bounded signed decimal value");
    Ok(Some(value.to_le_bytes()))
}

fn parse_float(input: &[u8], mut offset: usize) -> Result<Option<[u8; 4]>, DispatchError> {
    let start = offset;
    if matches!(input[offset], b'+' | b'-') {
        offset += 1;
    }
    let mut digits = 0;
    while matches!(input.get(offset), Some(b'0'..=b'9')) {
        offset += 1;
        digits += 1;
    }
    if input.get(offset) == Some(&b'.') {
        offset += 1;
        while matches!(input.get(offset), Some(b'0'..=b'9')) {
            offset += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return Ok(None);
    }
    if matches!(input.get(offset), Some(b'e' | b'E')) {
        offset += 1;
        if matches!(input.get(offset), Some(b'+' | b'-')) {
            offset += 1;
        }
        let exponent = offset;
        while matches!(input.get(offset), Some(b'0'..=b'9')) {
            offset += 1;
        }
        if exponent == offset {
            return Err(DispatchError::Unsupported);
        }
    }
    let token = std::str::from_utf8(&input[start..offset]).expect("ascii numeric token");
    let value: f32 = token.parse().map_err(|_| DispatchError::Unsupported)?;
    if !value.is_finite() {
        return Err(DispatchError::Unsupported);
    }
    Ok(Some(value.to_le_bytes()))
}

fn parse_unsigned(input: &[u8], mut offset: usize) -> Result<Option<[u8; 4]>, DispatchError> {
    let negative = match input[offset] {
        b'-' => {
            offset += 1;
            true
        }
        b'+' => {
            offset += 1;
            false
        }
        _ => false,
    };
    let start = offset;
    let mut magnitude = 0_u64;
    while matches!(input.get(offset), Some(b'0'..=b'9')) {
        magnitude = magnitude * 10 + u64::from(input[offset] - b'0');
        if magnitude > u64::from(u32::MAX) {
            return Err(DispatchError::Unsupported);
        }
        offset += 1;
    }
    if offset == start {
        return Ok(None);
    }
    let value = u32::try_from(magnitude).expect("bounded unsigned magnitude");
    let value = if negative {
        value.wrapping_neg()
    } else {
        value
    };
    Ok(Some(value.to_le_bytes()))
}

fn parse_hex(input: &[u8], mut offset: usize) -> Result<Option<[u8; 4]>, DispatchError> {
    let negative = match input[offset] {
        b'-' => {
            offset += 1;
            true
        }
        b'+' => {
            offset += 1;
            false
        }
        _ => false,
    };
    let prefixed =
        input.get(offset) == Some(&b'0') && matches!(input.get(offset + 1), Some(b'x' | b'X'));
    if prefixed {
        offset += 2;
    }
    let start = offset;
    let mut magnitude = 0_u64;
    while let Some(digit) = input
        .get(offset)
        .and_then(|byte| (*byte as char).to_digit(16))
    {
        magnitude = magnitude * 16 + u64::from(digit);
        if magnitude > u64::from(u32::MAX) {
            return Err(DispatchError::Unsupported);
        }
        offset += 1;
    }
    if offset == start {
        if prefixed {
            return Err(DispatchError::Unsupported);
        }
        return Ok(None);
    }
    let value = u32::try_from(magnitude).expect("bounded hex magnitude");
    let value = if negative {
        value.wrapping_neg()
    } else {
        value
    };
    Ok(Some(value.to_le_bytes()))
}

fn read_string(memory: &GuestMemory, pointer: u32) -> Result<Vec<u8>, DispatchError> {
    let mut bytes = Vec::new();
    for offset in 0..4096 {
        let address = pointer
            .checked_add(offset)
            .ok_or(MemoryError::AddressOverflow)?;
        let mut byte = [0];
        memory.read(u64::from(address), &mut byte)?;
        if byte[0] == 0 {
            return Ok(bytes);
        }
        bytes.push(byte[0]);
    }
    Err(DispatchError::Unsupported)
}
