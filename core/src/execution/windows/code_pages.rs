pub(super) const ANSI: u32 = 1252;
pub(super) const OEM: u32 = 437;

#[derive(Clone, Copy)]
pub(super) enum Call {
    Ansi,
    Oem,
    Info,
    WideToAnsi,
    AnsiToWide,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x88 => Some(Self::Ansi),
            0x8c => Some(Self::Oem),
            0x90 => Some(Self::Info),
            0x338 => Some(Self::WideToAnsi),
            0x33c => Some(Self::AnsiToWide),
            _ => None,
        }
    }

    pub(super) fn arguments(self) -> usize {
        match self {
            Self::Info => 2,
            Self::WideToAnsi => 8,
            Self::AnsiToWide => 6,
            Self::Ansi | Self::Oem => 0,
        }
    }

    pub(super) fn dispatch(
        self,
        arguments: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        match self {
            Self::Ansi => Ok(ANSI),
            Self::Oem => Ok(OEM),
            Self::Info => info(arguments[0], arguments[1], memory),
            Self::WideToAnsi => wide_to_ansi(arguments, memory),
            Self::AnsiToWide => ansi_to_wide(arguments, memory),
        }
    }
}

fn ansi_to_wide(args: &[u32], memory: &mut GuestMemory) -> Result<u32, DispatchError> {
    if !matches!(args[0], 0 | 3 | ANSI) || args[1] != 0 {
        return Err(DispatchError::Unsupported);
    }
    let [_, _, source, count, output, capacity] = args.try_into().unwrap();
    if source == 0
        || count == 0
        || count.cast_signed() < -1
        || capacity.cast_signed() < 0
        || (capacity != 0 && (output == 0 || output == source))
    {
        thread::set_last_error(memory, 87)?;
        return Ok(0);
    }
    let limit = if count == u32::MAX {
        65536
    } else {
        count as usize
    };
    if limit > 65536 {
        return Err(DispatchError::Unsupported);
    }
    let mut bytes = Vec::with_capacity(limit.min(256));
    for index in 0..limit {
        let address = source
            .checked_add(u32::try_from(index).map_err(|_| DispatchError::Unsupported)?)
            .ok_or(DispatchError::Unsupported)?;
        guest::check(memory, address, 1, Access::Read)?;
        let mut byte = [0];
        memory.read(u64::from(address), &mut byte)?;
        bytes.push(byte[0]);
        if count == u32::MAX && byte[0] == 0 {
            break;
        }
    }
    if count == u32::MAX && bytes.last().copied() != Some(0) {
        return Err(DispatchError::Unsupported);
    }
    let mut converted = Vec::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let unit = cp1252_unit(byte).ok_or(DispatchError::Unsupported)?;
        converted.extend_from_slice(&unit.to_le_bytes());
    }
    let units = u32::try_from(converted.len() / 2).map_err(|_| DispatchError::Unsupported)?;
    if capacity != 0 {
        if units > capacity {
            thread::set_last_error(memory, 122)?;
            return Ok(0);
        }
        guest::check(memory, output, converted.len(), Access::Write)?;
        memory.write(u64::from(output), &converted)?;
    }
    Ok(units)
}

fn cp1252_unit(byte: u8) -> Option<u16> {
    match byte {
        0x80 => Some(0x20ac),
        0x81 | 0x8d | 0x8f | 0x90 | 0x9d => None,
        0x82 => Some(0x201a),
        0x83 => Some(0x0192),
        0x84 => Some(0x201e),
        0x85 => Some(0x2026),
        0x86 => Some(0x2020),
        0x87 => Some(0x2021),
        0x88 => Some(0x02c6),
        0x89 => Some(0x2030),
        0x8a => Some(0x0160),
        0x8b => Some(0x2039),
        0x8c => Some(0x0152),
        0x8e => Some(0x017d),
        0x91 => Some(0x2018),
        0x92 => Some(0x2019),
        0x93 => Some(0x201c),
        0x94 => Some(0x201d),
        0x95 => Some(0x2022),
        0x96 => Some(0x2013),
        0x97 => Some(0x2014),
        0x98 => Some(0x02dc),
        0x99 => Some(0x2122),
        0x9a => Some(0x0161),
        0x9b => Some(0x203a),
        0x9c => Some(0x0153),
        0x9e => Some(0x017e),
        0x9f => Some(0x0178),
        _ => Some(u16::from(byte)),
    }
}

fn wide_to_ansi(args: &[u32], memory: &mut GuestMemory) -> Result<u32, DispatchError> {
    if !matches!(args[0], 0 | 3 | ANSI) || args[1] != 0 {
        return Err(DispatchError::Unsupported);
    }
    let [_, _, source, count, output, capacity, default, used] = args.try_into().unwrap();
    if source == 0
        || count == 0
        || count.cast_signed() < -1
        || capacity.cast_signed() < 0
        || (capacity != 0 && (output == 0 || output == source))
    {
        thread::set_last_error(memory, 87)?;
        return Ok(0);
    }
    let limit = if count == u32::MAX {
        65536
    } else {
        count as usize
    };
    if limit > 65536 {
        return Err(DispatchError::Unsupported);
    }
    let replacement = if default == 0 {
        b'?'
    } else {
        guest::check(memory, default, 1, Access::Read)?;
        let mut byte = [0];
        memory.read(u64::from(default), &mut byte)?;
        byte[0]
    };
    let mut units = Vec::with_capacity(limit.min(256));
    for index in 0..limit {
        let address = source
            .checked_add(u32::try_from(index).map_err(|_| DispatchError::Unsupported)? * 2)
            .ok_or(DispatchError::Unsupported)?;
        guest::check(memory, address, 2, Access::Read)?;
        let mut bytes = [0; 2];
        memory.read(u64::from(address), &mut bytes)?;
        let unit = u16::from_le_bytes(bytes);
        units.push(unit);
        if count == u32::MAX && unit == 0 {
            break;
        }
    }
    if count == u32::MAX && units.last().copied() != Some(0) {
        return Err(DispatchError::Unsupported);
    }
    let mut converted = Vec::with_capacity(units.len());
    let mut substituted = false;
    for character in char::decode_utf16(units) {
        let byte = character.ok().and_then(cp1252);
        converted.push(byte.unwrap_or_else(|| {
            substituted = true;
            replacement
        }));
    }
    if capacity != 0 {
        if converted.len() > capacity as usize {
            thread::set_last_error(memory, 122)?;
            return Ok(0);
        }
        guest::check(memory, output, converted.len(), Access::Write)?;
    }
    if used != 0 {
        guest::check(memory, used, 4, Access::Write)?;
    }
    if capacity != 0 {
        memory.write(u64::from(output), &converted)?;
    }
    if used != 0 {
        memory.write(u64::from(used), &u32::from(substituted).to_le_bytes())?;
    }
    u32::try_from(converted.len()).map_err(|_| DispatchError::Unsupported)
}

fn cp1252(character: char) -> Option<u8> {
    let value = u32::from(character);
    match value {
        0..=0x7f | 0xa0..=0xff => u8::try_from(value).ok(),
        0x20ac => Some(0x80),
        0x201a => Some(0x82),
        0x0192 => Some(0x83),
        0x201e => Some(0x84),
        0x2026 => Some(0x85),
        0x2020 => Some(0x86),
        0x2021 => Some(0x87),
        0x02c6 => Some(0x88),
        0x2030 => Some(0x89),
        0x0160 => Some(0x8a),
        0x2039 => Some(0x8b),
        0x0152 => Some(0x8c),
        0x017d => Some(0x8e),
        0x2018 => Some(0x91),
        0x2019 => Some(0x92),
        0x201c => Some(0x93),
        0x201d => Some(0x94),
        0x2022 => Some(0x95),
        0x2013 => Some(0x96),
        0x2014 => Some(0x97),
        0x02dc => Some(0x98),
        0x2122 => Some(0x99),
        0x0161 => Some(0x9a),
        0x203a => Some(0x9b),
        0x0153 => Some(0x9c),
        0x017e => Some(0x9e),
        0x0178 => Some(0x9f),
        _ => None,
    }
}

fn info(code_page: u32, output: u32, memory: &mut GuestMemory) -> Result<u32, DispatchError> {
    if output == 0 {
        thread::set_last_error(memory, 87)?;
        return Ok(0);
    }
    if !matches!(code_page, 0 | 1 | 3 | ANSI | OEM) {
        return Err(DispatchError::Unsupported);
    }
    // the two trailing structure padding bytes are not output fields.
    let mut fields = [0; 18];
    fields[0] = 1;
    fields[4] = b'?';
    guest::check(memory, output, fields.len(), Access::Write)?;
    memory.write(u64::from(output), &fields)?;
    Ok(1)
}
use super::super::Access;
use super::{DispatchError, GuestMemory, guest, thread};
