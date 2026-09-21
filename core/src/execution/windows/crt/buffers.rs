use super::{Access, DispatchError, GuestMemory, guest};

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
