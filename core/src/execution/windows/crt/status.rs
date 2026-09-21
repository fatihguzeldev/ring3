use super::{Access, DispatchError, ERRNO, GuestMemory, directory, guest};

pub(super) fn query(
    directory: &directory::Directory,
    memory: &mut GuestMemory,
    source: u32,
    output: u32,
) -> Result<u32, DispatchError> {
    let Some(status) = directory.legacy_status(memory, source)? else {
        guest::write_word(memory, ERRNO, 2)?;
        return Ok(u32::MAX);
    };
    let size = i32::try_from(status.size).map_err(|_| DispatchError::Unsupported)?;
    let mode: u16 = if status.directory {
        0x41ff
    } else if status.executable {
        0x81ff
    } else {
        0x81b6
    };
    let mut record = [0; 36];
    record[0] = status.drive;
    record[6..8].copy_from_slice(&mode.to_le_bytes());
    record[8] = 1;
    record[16] = status.drive;
    record[20..24].copy_from_slice(&size.to_le_bytes());
    guest::check(memory, output, record.len(), Access::Write)?;
    memory.write(u64::from(output), &record)?;
    Ok(0)
}
