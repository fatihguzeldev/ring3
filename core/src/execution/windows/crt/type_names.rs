use super::{Access, DispatchError, ERRNO, GuestMemory, MemoryError, guest, heap};

pub(super) fn name(
    memory: &mut GuestMemory,
    heap: &mut heap::Heap,
    this: u32,
) -> Result<u32, DispatchError> {
    let cache = this.checked_add(4).ok_or(MemoryError::AddressOverflow)?;
    let mut pointer = [0];
    guest::read_words(memory, cache, &mut pointer)?;
    if pointer[0] != 0 {
        return Ok(pointer[0]);
    }
    let source = this.checked_add(8).ok_or(MemoryError::AddressOverflow)?;
    let raw = read_name(memory, source)?;
    let output = decode(&raw).ok_or(DispatchError::Unsupported)?;
    guest::check(memory, cache, 4, Access::Write)?;
    let length = u32::try_from(output.len()).expect("bounded type name fits u32");
    let Some(pointer) = heap.allocate_crt(length, memory)? else {
        guest::write_word(memory, ERRNO, 12)?;
        return Ok(0);
    };
    memory
        .write(u64::from(pointer), &output)
        .expect("new crt allocation is writable");
    guest::write_word(memory, cache, pointer).expect("cache write was checked");
    Ok(pointer)
}

fn read_name(memory: &GuestMemory, source: u32) -> Result<Vec<u8>, DispatchError> {
    let mut bytes = Vec::new();
    for offset in 0..1024 {
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

fn decode(raw: &[u8]) -> Option<Vec<u8>> {
    let raw = raw.strip_prefix(b".?A")?;
    let (prefix, raw): (&[u8], &[u8]) = match raw {
        [b'V', rest @ ..] => (b"class ", rest),
        [b'U', rest @ ..] => (b"struct ", rest),
        [b'T', rest @ ..] => (b"union ", rest),
        [b'W', b'4', rest @ ..] => (b"enum ", rest),
        _ => return None,
    };
    let names: Vec<_> = raw
        .strip_suffix(b"@@")?
        .split(|byte| *byte == b'@')
        .collect();
    if names.len() > 32 || !names.iter().all(|name| identifier(name)) {
        return None;
    }
    let mut output = prefix.to_vec();
    for (index, name) in names.iter().rev().enumerate() {
        if index != 0 {
            output.extend_from_slice(b"::");
        }
        output.extend_from_slice(name);
    }
    output.push(0);
    Some(output)
}

fn identifier(name: &[u8]) -> bool {
    name.first()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
        && name
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}
