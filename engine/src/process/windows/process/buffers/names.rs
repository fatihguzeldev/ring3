use super::*;
use crate::memory::MemoryError;

#[derive(Clone, Copy)]
pub(in crate::process::windows) enum NameBuffer32 {
    User,
    Computer,
}

pub(in crate::process::windows) fn write_name(
    memory: &mut AddressSpace,
    name: &str,
    output: u32,
    size: u32,
    wide: bool,
    code_page: CodePage32,
    kind: NameBuffer32,
) -> Result<Option<u32>, MemoryError> {
    let mut capacity = [0; 4];
    memory.read(GuestAddress(size), &mut capacity)?;
    let capacity = u32::from_le_bytes(capacity);
    let mut bytes = encoded(name, wide, code_page);
    let width = if wide { 2 } else { 1 };
    let length = (bytes.len() / width) as u32;
    let required = length + 1;
    if capacity < required {
        memory.write(GuestAddress(size), &required.to_le_bytes())?;
        return Ok(Some(match kind {
            NameBuffer32::User => 122,
            NameBuffer32::Computer => 111,
        }));
    }
    bytes.resize(bytes.len() + width, 0);
    let copied = match kind {
        NameBuffer32::User => required,
        NameBuffer32::Computer => length,
    };
    memory.write_spans(&[
        (GuestAddress(output), &bytes),
        (GuestAddress(size), &copied.to_le_bytes()),
    ])?;
    Ok(None)
}
