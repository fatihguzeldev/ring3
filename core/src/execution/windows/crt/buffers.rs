use super::{Access, DispatchError, GuestMemory, guest};

pub(super) fn fill(
    memory: &mut GuestMemory,
    destination: u32,
    value: u32,
    count: u32,
) -> Result<u32, DispatchError> {
    let length = usize::try_from(count).expect("u32 count fits target usize");
    guest::check(memory, destination, length, Access::Write)?;
    memory.fill(u64::from(destination), length, value.to_le_bytes()[0])?;
    Ok(destination)
}

pub(super) fn copy(
    memory: &mut GuestMemory,
    destination: u32,
    source: u32,
    count: u32,
) -> Result<u32, DispatchError> {
    if count == 0 {
        return Ok(destination);
    }
    let length = usize::try_from(count).expect("guest32 count fits usize");
    guest::check(memory, source, length, Access::Read)?;
    guest::check(memory, destination, length, Access::Write)?;
    let (src, dst, size) = (u64::from(source), u64::from(destination), u64::from(count));
    if src < dst + size && dst < src + size {
        return Err(DispatchError::Unsupported);
    }
    let mut bytes = [0; 4096];
    for offset in (0..length).step_by(bytes.len()) {
        let size = (length - offset).min(bytes.len());
        memory
            .read(src + offset as u64, &mut bytes[..size])
            .expect("nonoverlapping source range was checked");
        memory
            .write(dst + offset as u64, &bytes[..size])
            .expect("destination range was checked");
    }
    Ok(destination)
}

pub(super) fn move_bytes(
    memory: &mut GuestMemory,
    destination: u32,
    source: u32,
    count: u32,
) -> Result<u32, DispatchError> {
    if count == 0 {
        return Ok(destination);
    }
    let length = usize::try_from(count).expect("guest32 count fits usize");
    guest::check(memory, source, length, Access::Read)?;
    guest::check(memory, destination, length, Access::Write)?;
    if source == destination {
        return Ok(destination);
    }

    let (src, dst) = (u64::from(source), u64::from(destination));
    let mut bytes = [0; 4096];
    if src < dst && dst < src + u64::from(count) {
        let mut end = length;
        while end != 0 {
            let size = end.min(bytes.len());
            let offset = end - size;
            memory
                .read(src + offset as u64, &mut bytes[..size])
                .expect("source range was checked");
            memory
                .write(dst + offset as u64, &bytes[..size])
                .expect("destination range was checked");
            end = offset;
        }
    } else {
        for offset in (0..length).step_by(bytes.len()) {
            let size = (length - offset).min(bytes.len());
            memory
                .read(src + offset as u64, &mut bytes[..size])
                .expect("source range was checked");
            memory
                .write(dst + offset as u64, &bytes[..size])
                .expect("destination range was checked");
        }
    }
    Ok(destination)
}

pub(super) fn compare(
    memory: &GuestMemory,
    left: u32,
    right: u32,
    count: u32,
) -> Result<u32, DispatchError> {
    if count > 65536 {
        return Err(DispatchError::Unsupported);
    }
    let length = usize::try_from(count).expect("bounded count fits usize");
    guest::check(memory, left, length, Access::Read)?;
    guest::check(memory, right, length, Access::Read)?;
    let (mut a, mut b) = ([0; 256], [0; 256]);
    for offset in (0..length).step_by(a.len()) {
        let size = (length - offset).min(a.len());
        memory.read(u64::from(left) + offset as u64, &mut a[..size])?;
        memory.read(u64::from(right) + offset as u64, &mut b[..size])?;
        for (&a, &b) in a[..size].iter().zip(&b[..size]) {
            if a != b {
                return Ok((i32::from(a) - i32::from(b)).cast_unsigned());
            }
        }
    }
    Ok(0)
}

pub(super) fn find(
    memory: &GuestMemory,
    source: u32,
    value: u32,
    count: u32,
) -> Result<u32, DispatchError> {
    if count > 65536 {
        return Err(DispatchError::Unsupported);
    }
    if count == 0 {
        return Ok(0);
    }
    let length = usize::try_from(count).expect("bounded count fits usize");
    guest::check(memory, source, length, Access::Read)?;
    let target = value.to_le_bytes()[0];
    let mut bytes = [0; 256];
    for offset in (0..length).step_by(bytes.len()) {
        let size = (length - offset).min(bytes.len());
        memory.read(u64::from(source) + offset as u64, &mut bytes[..size])?;
        if let Some(index) = bytes[..size].iter().position(|&byte| byte == target) {
            return Ok(source + u32::try_from(offset + index).expect("bounded offset fits u32"));
        }
    }
    Ok(0)
}
