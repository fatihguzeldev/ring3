use super::{Access, DispatchError, ERRNO, GuestMemory, MemoryError, guest};

pub(super) fn sscanf_decimal(
    memory: &mut GuestMemory,
    args: &[u32],
    stack: u32,
) -> Result<u32, DispatchError> {
    if args[0] == 0 || args[1] == 0 {
        guest::write_word(memory, ERRNO, 22)?;
        return Ok(u32::MAX);
    }
    if read_string(memory, args[1])? != b"%d" {
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
        return Ok(0);
    }
    let signed = i64::try_from(magnitude).expect("bounded decimal magnitude");
    let value = i32::try_from(if negative { -signed } else { signed })
        .expect("bounded signed decimal value");
    let address = stack.checked_add(12).ok_or(MemoryError::AddressOverflow)?;
    let mut destination = [0];
    guest::read_words(memory, address, &mut destination)?;
    guest::check(memory, destination[0], 4, Access::Write)?;
    memory.write(u64::from(destination[0]), &value.to_le_bytes())?;
    Ok(1)
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
