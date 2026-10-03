use super::{CallFrame32, CallingConvention32, FrameError};
use crate::cpu::x86::Register32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum WindowsApi32 {
    GetLastError = 0x0001_0001,
    SetLastError = 0x0001_0002,
}

impl WindowsApi32 {
    pub fn resolve(module: &str, symbol: &str) -> Option<Self> {
        if !module.eq_ignore_ascii_case("kernel32.dll") {
            return None;
        }
        match symbol {
            "GetLastError" => Some(Self::GetLastError),
            "SetLastError" => Some(Self::SetLastError),
            _ => None,
        }
    }

    pub fn from_id(id: u32) -> Option<Self> {
        match id {
            0x0001_0001 => Some(Self::GetLastError),
            0x0001_0002 => Some(Self::SetLastError),
            _ => None,
        }
    }

    pub fn id(self) -> u32 {
        self as u32
    }

    pub fn convention(self) -> CallingConvention32 {
        CallingConvention32::Stdcall
    }

    pub fn stack_words(self) -> u32 {
        match self {
            Self::GetLastError => 0,
            Self::SetLastError => 1,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct ThreadState32 {
    last_error: u32,
}

impl ThreadState32 {
    pub(crate) fn prepare(
        self,
        api: WindowsApi32,
        frame: &CallFrame32,
    ) -> Result<(u32, Self), FrameError> {
        if frame.convention() != api.convention() || frame.stack_words() != api.stack_words() {
            return Err(FrameError::InvalidRequest);
        }
        Ok(match api {
            WindowsApi32::GetLastError => (self.last_error, self),
            WindowsApi32::SetLastError => (
                frame.state().registers[Register32::Eax.index()],
                Self {
                    last_error: frame.arguments()[0],
                },
            ),
        })
    }
}
