use super::super::Access;
use super::{DispatchError, GuestMemory, gdi, guest, thread};

pub(super) const SYSTEM_DIRECTORY: &[u8] = b"C:\\Windows\\System32";
const COMPUTER_NAME: &[u8; 6] = b"RING3\0";

const LARGE_ICON: u32 = 32;
const SMALL_ICON: u32 = 16;
const SCROLL_WIDTH: u32 = 16;
const SCROLL_HEIGHT: u32 = 16;

fn metrics(index: u32) -> Result<u32, DispatchError> {
    match index {
        0 => Ok(gdi::SCREEN_WIDTH),
        1 => Ok(gdi::SCREEN_HEIGHT),
        2 | 3 => Ok(SCROLL_WIDTH),
        9 | 10 | 20 | 21 => Ok(SCROLL_HEIGHT),
        11 | 12 => Ok(LARGE_ICON),
        49 | 50 => Ok(SMALL_ICON),
        _ => Err(DispatchError::Unsupported),
    }
}

#[derive(Clone, Copy)]
pub(super) enum Call {
    Metrics,
    Color,
    Directory,
    ComputerName,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x9c => Some(Self::Metrics),
            0xac => Some(Self::Color),
            0xf8 => Some(Self::Directory),
            0x20c => Some(Self::ComputerName),
            _ => None,
        }
    }

    pub(super) fn arguments(self) -> usize {
        if matches!(self, Self::Directory | Self::ComputerName) {
            2
        } else {
            1
        }
    }

    pub(super) fn dispatch(
        self,
        arguments: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        match self {
            Self::Metrics => metrics(arguments[0]),
            Self::Color => color(arguments[0]),
            Self::Directory => directory(memory, arguments[0], arguments[1]),
            Self::ComputerName => computer_name(memory, arguments[0], arguments[1]),
        }
    }
}

fn computer_name(memory: &mut GuestMemory, output: u32, size: u32) -> Result<u32, DispatchError> {
    let mut capacity = [0];
    guest::read_words(memory, size, &mut capacity)?;
    guest::check(memory, size, 4, Access::Write)?;
    let required = u32::try_from(COMPUTER_NAME.len()).expect("fixed name length fits u32");
    if capacity[0] < required {
        thread::check_last_error_write(memory)?;
        guest::write_word(memory, size, required)?;
        thread::set_last_error(memory, 111)?;
        return Ok(0);
    }
    guest::check(memory, output, COMPUTER_NAME.len(), Access::Write)?;
    memory.write(u64::from(output), COMPUTER_NAME)?;
    guest::write_word(memory, size, required - 1)?;
    Ok(1)
}

fn directory(memory: &mut GuestMemory, output: u32, capacity: u32) -> Result<u32, DispatchError> {
    let mut bytes = [0; SYSTEM_DIRECTORY.len() + 1];
    let required = u32::try_from(bytes.len()).expect("fixed directory length fits u32");
    if capacity < required {
        return Ok(required);
    }
    guest::check(memory, output, bytes.len(), Access::Write)?;
    bytes[..SYSTEM_DIRECTORY.len()].copy_from_slice(SYSTEM_DIRECTORY);
    memory.write(u64::from(output), &bytes)?;
    Ok(required - 1)
}

pub(super) fn color(index: u32) -> Result<u32, DispatchError> {
    match index {
        0 | 4 | 10 | 11 | 15 | 19 | 22 | 30 => Ok(0x00c8_d0d4),
        1 => Ok(0x00a5_6e3a),
        2 | 13 | 29 => Ok(0x006a_240a),
        3 | 12 | 16 | 17 => Ok(0x0080_8080),
        5 | 9 | 14 | 20 => Ok(0x00ff_ffff),
        21 => Ok(0x0040_4040),
        24 => Ok(0x00e1_ffff),
        26 => Ok(0x00c8_0000),
        27 => Ok(0x00f0_caa6),
        28 => Ok(0x00c0_c0c0),
        25 => Err(DispatchError::Unsupported),
        // black elements and out-of-range indices both return zero.
        _ => Ok(0),
    }
}
