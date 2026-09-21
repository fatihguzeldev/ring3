use super::{Access, DispatchError, GuestMemory, MemoryError, guest};

const OUTPUT_LIMIT: usize = 65536;

pub(super) fn write(memory: &mut GuestMemory, arguments: &[u32]) -> Result<u32, DispatchError> {
    let (destination, capacity, format, values) =
        (arguments[0], arguments[1], arguments[2], arguments[3]);
    if destination == 0 && capacity != 0 {
        return Err(DispatchError::Unsupported);
    }
    let format = read_string(memory, format, 4096)?;
    let mut output = render(memory, &format, values)?;
    let length = u32::try_from(output.len()).expect("bounded output fits u32");
    if destination == 0 {
        return Ok(length);
    }
    if length < capacity {
        output.push(0);
    } else {
        output.truncate(usize::try_from(capacity).expect("u32 capacity fits usize"));
    }
    guest::check(memory, destination, output.len(), Access::Write)?;
    memory.write(u64::from(destination), &output)?;
    Ok(if length > capacity { u32::MAX } else { length })
}

fn render(memory: &GuestMemory, format: &[u8], values: u32) -> Result<Vec<u8>, DispatchError> {
    let mut cursor = u64::from(values);
    let mut output = Vec::new();
    let mut bytes = format.iter().copied();
    while let Some(byte) = bytes.next() {
        if byte != b'%' {
            append(&mut output, &[byte])?;
            continue;
        }
        let conversion = bytes.next().ok_or(DispatchError::Unsupported)?;
        if conversion == b'%' {
            append(&mut output, b"%")?;
            continue;
        }
        if !matches!(conversion, b's' | b'c' | b'd' | b'i' | b'u' | b'x' | b'X') {
            return Err(DispatchError::Unsupported);
        }
        let address = u32::try_from(cursor).map_err(|_| MemoryError::AddressOverflow)?;
        let mut value = [0];
        guest::read_words(memory, address, &mut value)?;
        cursor += 4;
        let value = value[0];
        let text = match conversion {
            b's' => read_string(memory, value, OUTPUT_LIMIT)?,
            b'c' => vec![value.to_le_bytes()[0]],
            b'd' | b'i' => value.cast_signed().to_string().into_bytes(),
            b'u' => value.to_string().into_bytes(),
            b'x' => format!("{value:x}").into_bytes(),
            b'X' => format!("{value:X}").into_bytes(),
            _ => unreachable!("conversion was validated"),
        };
        append(&mut output, &text)?;
    }
    Ok(output)
}

fn append(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), DispatchError> {
    if bytes.len() > OUTPUT_LIMIT - output.len() {
        return Err(DispatchError::Unsupported);
    }
    output.extend_from_slice(bytes);
    Ok(())
}

fn read_string(memory: &GuestMemory, source: u32, limit: usize) -> Result<Vec<u8>, DispatchError> {
    if source == 0 {
        return Err(DispatchError::Unsupported);
    }
    let mut bytes = Vec::new();
    for offset in 0..u32::try_from(limit).expect("bounded string limit fits u32") {
        let address = source
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
