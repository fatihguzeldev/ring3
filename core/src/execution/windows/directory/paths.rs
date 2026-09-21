use super::{DispatchError, GuestMemory, MemoryError};

pub(super) enum PathError {
    Unsupported,
    Windows(u32),
}

pub(super) fn read(memory: &GuestMemory, source: u32) -> Result<Vec<u8>, DispatchError> {
    let mut bytes = Vec::with_capacity(32768);
    for offset in 0..32768 {
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

pub(super) fn resolve(current: &[u8], input: &[u8]) -> Result<Vec<u8>, PathError> {
    if input.is_empty() {
        return Err(PathError::Windows(3));
    }
    if input.starts_with(b"\\\\") {
        return Err(PathError::Unsupported);
    }
    let (base, tail) = if input.get(1) == Some(&b':') {
        if !input[0].is_ascii_alphabetic() {
            return Err(PathError::Windows(123));
        }
        if input.get(2) != Some(&b'\\') {
            return Err(PathError::Unsupported);
        }
        (&input[..3], &input[3..])
    } else if input[0] == b'\\' {
        (&current[..3], &input[1..])
    } else {
        (current, input)
    };
    for component in tail.split(|&b| b == b'\\') {
        if !matches!(component, b"" | b"." | b"..") {
            validate_component(component)?;
        }
    }
    let mut result = Vec::with_capacity(32768);
    result.extend_from_slice(&base[..3]);
    let mut parents = 0;
    // resolve parents before sizing the final path, without storing a long intermediate path.
    for component in base[3..]
        .split(|&b| b == b'\\')
        .chain(tail.split(|&b| b == b'\\'))
        .rev()
    {
        match component {
            b"" | b"." => {}
            b".." => parents += 1,
            _ if parents > 0 => parents -= 1,
            _ => {
                let separator = usize::from(result.len() > 3);
                if result.len() + separator + component.len() > 32767 {
                    return Err(PathError::Windows(206));
                }
                if separator != 0 {
                    result.push(b'\\');
                }
                result.extend(component.iter().rev().copied());
            }
        }
    }
    result[3..].reverse();
    Ok(result)
}

pub(super) fn validate_component(component: &[u8]) -> Result<(), PathError> {
    if component.iter().any(|&b| b >= 0x7f || b == b'/')
        || matches!(component.last(), Some(b'.' | b' '))
        || reserved_device(component)
    {
        return Err(PathError::Unsupported);
    }
    if component
        .iter()
        .any(|&b| b < 0x20 || b"<>:\"|?*".contains(&b))
    {
        return Err(PathError::Windows(123));
    }
    Ok(())
}

fn reserved_device(component: &[u8]) -> bool {
    let stem = component
        .split(|&b| b == b'.')
        .next()
        .expect("nonempty component");
    [b"CON", b"PRN", b"AUX", b"NUL"]
        .iter()
        .any(|name| stem.eq_ignore_ascii_case(*name))
        || (stem.len() == 4
            && (stem[..3].eq_ignore_ascii_case(b"COM") || stem[..3].eq_ignore_ascii_case(b"LPT"))
            && (b'1'..=b'9').contains(&stem[3]))
}

#[cfg(test)]
mod tests {
    use super::resolve;

    #[test]
    fn normalized_components_match_a_forward_stack_oracle() {
        let tokens = [&b"a"[..], b"b", b".", b".."];
        for base in [&b"C:\\"[..], b"C:\\Root", b"C:\\Root\\Leaf"] {
            for length in 1..=5_u32 {
                for mut pattern in 0..4_usize.pow(length) {
                    let mut input = Vec::new();
                    let mut expected: Vec<&[u8]> = base[3..]
                        .split(|&b| b == b'\\')
                        .filter(|part| !part.is_empty())
                        .collect();
                    for index in 0..length {
                        let token = tokens[pattern % 4];
                        pattern /= 4;
                        if index > 0 {
                            input.push(b'\\');
                        }
                        input.extend_from_slice(token);
                        match token {
                            b"." => {}
                            b".." => {
                                expected.pop();
                            }
                            _ => expected.push(token),
                        }
                    }
                    let mut bytes = b"C:\\".to_vec();
                    bytes.extend_from_slice(&expected.join(&b'\\'));
                    let Ok(actual) = resolve(base, &input) else {
                        panic!("valid path rejected");
                    };
                    assert_eq!(actual, bytes, "base={base:?} input={input:?}");
                }
            }
        }
    }
}
