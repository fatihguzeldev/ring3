use super::{DispatchError, GuestMemory, thread};

const SYSTEM_IDS: [u32; 14] = [
    32512, 32513, 32514, 32515, 32516, 32642, 32643, 32644, 32645, 32646, 32648, 32649, 32650,
    32651,
];
const HANDLE_BASE: u32 = 0x4000_0000;

#[derive(Clone, Copy)]
pub(super) enum Call {
    Load,
    Set,
    Get,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0xbc => Some(Self::Load),
            0xc0 => Some(Self::Set),
            0xc4 => Some(Self::Get),
            _ => None,
        }
    }

    pub(super) fn arguments(self) -> usize {
        match self {
            Self::Load => 2,
            Self::Set => 1,
            Self::Get => 0,
        }
    }
}

#[derive(Default)]
pub(super) struct Cursors {
    loaded: [bool; 14],
    current: u32,
}

impl Cursors {
    pub(super) fn dispatch(
        &mut self,
        call: Call,
        arguments: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        match call {
            Call::Load => {
                if arguments[0] != 0 {
                    return Err(DispatchError::Unsupported);
                }
                let index = SYSTEM_IDS
                    .iter()
                    .position(|id| *id == arguments[1])
                    .ok_or(DispatchError::Unsupported)?;
                self.loaded[index] = true;
                Ok(HANDLE_BASE + u32::try_from(index).expect("system cursor slot fits u32") * 4)
            }
            Call::Set => {
                let handle = arguments[0];
                if handle != 0 && !self.contains(handle) {
                    thread::set_last_error(memory, 1402)?;
                    return Ok(0);
                }
                Ok(std::mem::replace(&mut self.current, handle))
            }
            Call::Get => Ok(self.current),
        }
    }

    fn contains(&self, handle: u32) -> bool {
        handle.checked_sub(HANDLE_BASE).is_some_and(|offset| {
            offset.is_multiple_of(4) && self.loaded.get((offset / 4) as usize) == Some(&true)
        })
    }
}
