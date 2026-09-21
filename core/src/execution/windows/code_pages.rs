pub(super) const ANSI: u32 = 1252;
pub(super) const OEM: u32 = 437;

#[derive(Clone, Copy)]
pub(super) enum Call {
    Ansi,
    Oem,
    Info,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x88 => Some(Self::Ansi),
            0x8c => Some(Self::Oem),
            0x90 => Some(Self::Info),
            _ => None,
        }
    }

    pub(super) fn arguments(self) -> usize {
        if matches!(self, Self::Info) { 2 } else { 0 }
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
        }
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
