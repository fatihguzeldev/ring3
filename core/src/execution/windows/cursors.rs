use super::super::Access;
use super::{DispatchError, GuestMemory, gdi, guest, thread};

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
    GetPosition,
    SetPosition,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0xbc => Some(Self::Load),
            0xc0 => Some(Self::Set),
            0xc4 => Some(Self::Get),
            0xcc => Some(Self::GetPosition),
            0xd0 => Some(Self::SetPosition),
            _ => None,
        }
    }

    pub(super) fn arguments(self) -> usize {
        match self {
            Self::Load | Self::SetPosition => 2,
            Self::Set | Self::GetPosition => 1,
            Self::Get => 0,
        }
    }
}

#[derive(Default)]
pub(super) struct Cursors {
    loaded: [bool; 14],
    current: u32,
    position: [u32; 2],
}

impl Cursors {
    pub(super) fn position(&self) -> [i32; 2] {
        self.position.map(u32::cast_signed)
    }

    pub(super) fn dispatch(
        &mut self,
        call: Call,
        arguments: &[u32],
        teb: thread::Teb,
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
                    teb.set_last_error(memory, 1402)?;
                    return Ok(0);
                }
                Ok(std::mem::replace(&mut self.current, handle))
            }
            Call::Get => Ok(self.current),
            Call::GetPosition => {
                let output = arguments[0];
                if output == 0 {
                    teb.set_last_error(memory, 998)?;
                    return Ok(0);
                }
                guest::check(memory, output, 8, Access::Write)?;
                let mut bytes = [0; 8];
                bytes[..4].copy_from_slice(&self.position[0].to_le_bytes());
                bytes[4..].copy_from_slice(&self.position[1].to_le_bytes());
                memory.write(u64::from(output), &bytes)?;
                Ok(1)
            }
            Call::SetPosition => {
                let clamp = |value: u32, extent: u32| {
                    value
                        .cast_signed()
                        .clamp(0, i32::try_from(extent - 1).expect("screen bound fits i32"))
                        .cast_unsigned()
                };
                self.position = [
                    clamp(arguments[0], gdi::SCREEN_WIDTH),
                    clamp(arguments[1], gdi::SCREEN_HEIGHT),
                ];
                Ok(1)
            }
        }
    }

    fn contains(&self, handle: u32) -> bool {
        handle.checked_sub(HANDLE_BASE).is_some_and(|offset| {
            offset.is_multiple_of(4) && self.loaded.get((offset / 4) as usize) == Some(&true)
        })
    }
}
