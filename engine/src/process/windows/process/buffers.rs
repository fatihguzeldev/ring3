use crate::{
    memory::{Access, AddressSpace, GuestAddress},
    windows::{process::ProcessError32, text::CodePage32},
};

pub(in crate::process::windows) mod names;

pub(super) fn string(
    memory: &AddressSpace,
    pointer: u32,
    wide: bool,
    code_page: CodePage32,
) -> Result<String, ProcessError32> {
    if pointer == 0 {
        return Err(ProcessError32::InvalidParameter);
    }
    let mut bytes = Vec::new();
    for index in 0..32768u32 {
        let at = pointer
            .checked_add(index * if wide { 2 } else { 1 })
            .ok_or(ProcessError32::NoAccess)?;
        let mut unit = [0; 2];
        memory
            .read(GuestAddress(at), &mut unit[..if wide { 2 } else { 1 }])
            .map_err(|_| ProcessError32::NoAccess)?;
        if u16::from_le_bytes(unit) == 0 {
            let units = if wide {
                bytes
                    .chunks_exact(2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                    .collect::<Vec<_>>()
            } else {
                code_page
                    .decode(&bytes, false)
                    .map_err(|_| ProcessError32::InvalidParameter)?
            };
            return String::from_utf16(&units).map_err(|_| ProcessError32::InvalidParameter);
        }
        bytes.extend_from_slice(&unit[..if wide { 2 } else { 1 }]);
    }
    Err(ProcessError32::InvalidParameter)
}
pub(super) fn encoded(value: &str, wide: bool, code_page: CodePage32) -> Vec<u8> {
    let units = value.encode_utf16().collect::<Vec<_>>();
    if wide {
        units.into_iter().flat_map(u16::to_le_bytes).collect()
    } else {
        code_page
            .encode(&units, false)
            .expect("replacement conversion always succeeds")
            .0
    }
}
pub(super) fn write(
    memory: &mut AddressSpace,
    pointer: u32,
    bytes: &[u8],
) -> Result<(), ProcessError32> {
    if pointer == 0 && !bytes.is_empty() {
        return Err(ProcessError32::NoAccess);
    }
    memory
        .check_span(GuestAddress(pointer), bytes.len(), Access::Write)
        .map_err(|_| ProcessError32::NoAccess)?;
    memory
        .write(GuestAddress(pointer), bytes)
        .map_err(|_| ProcessError32::NoAccess)
}
pub(super) fn counted(
    memory: &mut AddressSpace,
    pointer: u32,
    capacity: u32,
    value: &str,
    wide: bool,
    code_page: CodePage32,
) -> Result<u32, ProcessError32> {
    let mut bytes = encoded(value, wide, code_page);
    let width = if wide { 2 } else { 1 };
    let count = bytes.len() / width;
    if capacity as usize <= count {
        return Ok((count + 1) as u32);
    }
    bytes.resize(bytes.len() + width, 0);
    write(memory, pointer, &bytes)?;
    Ok(count as u32)
}
