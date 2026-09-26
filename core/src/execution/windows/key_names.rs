use super::{Access, DispatchError, GuestMemory, guest};

pub(super) fn get(
    memory: &mut GuestMemory,
    parameter: u32,
    output: u32,
    capacity: u32,
) -> Result<u32, DispatchError> {
    if capacity.cast_signed() <= 0 {
        return Ok(0);
    }
    let scan = ((parameter >> 16) & 0xff) as u8;
    let extended = parameter & (1 << 24) != 0;
    let name: &[u8] = if scan == 0 || scan > 0x87 {
        b""
    } else {
        us_key_name(scan, extended).ok_or(DispatchError::Unsupported)?
    };
    let count = name.len().min((capacity - 1) as usize);
    guest::check(memory, output, count + 1, Access::Write)?;
    let mut bytes = [0; 16];
    bytes[..count].copy_from_slice(&name[..count]);
    memory.write(u64::from(output), &bytes[..=count])?;
    Ok(u32::try_from(count).expect("fixed key name length fits u32"))
}

fn us_key_name(scan: u8, extended: bool) -> Option<&'static [u8]> {
    if extended {
        return match scan {
            0x48 => Some(b"Up"),
            0x4b => Some(b"Left"),
            0x4d => Some(b"Right"),
            0x50 => Some(b"Down"),
            0x47 => Some(b"Home"),
            0x4f => Some(b"End"),
            0x49 => Some(b"Page Up"),
            0x51 => Some(b"Page Down"),
            0x52 => Some(b"Insert"),
            0x53 => Some(b"Delete"),
            _ => None,
        };
    }
    match scan {
        0x01 => Some(b"Esc"),
        0x02 => Some(b"1"),
        0x03 => Some(b"2"),
        0x04 => Some(b"3"),
        0x05 => Some(b"4"),
        0x06 => Some(b"5"),
        0x07 => Some(b"6"),
        0x08 => Some(b"7"),
        0x09 => Some(b"8"),
        0x0a => Some(b"9"),
        0x0b => Some(b"0"),
        0x0e => Some(b"Backspace"),
        0x0f => Some(b"Tab"),
        0x10 => Some(b"Q"),
        0x11 => Some(b"W"),
        0x12 => Some(b"E"),
        0x13 => Some(b"R"),
        0x14 => Some(b"T"),
        0x15 => Some(b"Y"),
        0x16 => Some(b"U"),
        0x17 => Some(b"I"),
        0x18 => Some(b"O"),
        0x19 => Some(b"P"),
        0x1c => Some(b"Enter"),
        0x1e => Some(b"A"),
        0x1f => Some(b"S"),
        0x20 => Some(b"D"),
        0x21 => Some(b"F"),
        0x22 => Some(b"G"),
        0x23 => Some(b"H"),
        0x24 => Some(b"J"),
        0x25 => Some(b"K"),
        0x26 => Some(b"L"),
        0x2c => Some(b"Z"),
        0x2d => Some(b"X"),
        0x2e => Some(b"C"),
        0x2f => Some(b"V"),
        0x30 => Some(b"B"),
        0x31 => Some(b"N"),
        0x32 => Some(b"M"),
        0x39 => Some(b"Space"),
        0x3b => Some(b"F1"),
        0x3c => Some(b"F2"),
        0x3d => Some(b"F3"),
        0x3e => Some(b"F4"),
        0x3f => Some(b"F5"),
        0x40 => Some(b"F6"),
        0x41 => Some(b"F7"),
        0x42 => Some(b"F8"),
        0x43 => Some(b"F9"),
        0x44 => Some(b"F10"),
        0x57 => Some(b"F11"),
        0x58 => Some(b"F12"),
        _ => None,
    }
}
