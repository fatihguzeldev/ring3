use super::{CallFrame32, CallingConvention32, FrameError};
use crate::{cpu::x86::Register32, memory::GuestAddress};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum WindowsApi32 {
    GetLastError = 0x0001_0001,
    SetLastError = 0x0001_0002,
    ExitProcess = 0x0001_0003,
    GetModuleHandleA = 0x0001_0004,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WindowsOutcome32 {
    Return(u32),
    ExitProcess(u32),
}

impl WindowsApi32 {
    pub fn resolve(module: &str, symbol: &str) -> Option<Self> {
        if !module.eq_ignore_ascii_case("kernel32.dll") {
            return None;
        }
        match symbol {
            "GetLastError" => Some(Self::GetLastError),
            "SetLastError" => Some(Self::SetLastError),
            "ExitProcess" => Some(Self::ExitProcess),
            "GetModuleHandleA" => Some(Self::GetModuleHandleA),
            _ => None,
        }
    }

    pub fn from_id(id: u32) -> Option<Self> {
        match id {
            0x0001_0001 => Some(Self::GetLastError),
            0x0001_0002 => Some(Self::SetLastError),
            0x0001_0003 => Some(Self::ExitProcess),
            0x0001_0004 => Some(Self::GetModuleHandleA),
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
            Self::SetLastError | Self::ExitProcess | Self::GetModuleHandleA => 1,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct ProcessContext32 {
    pub main_image_base: Option<GuestAddress>,
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
        context: ProcessContext32,
    ) -> Result<(WindowsOutcome32, Self), FrameError> {
        if frame.convention() != api.convention() || frame.stack_words() != api.stack_words() {
            return Err(FrameError::InvalidRequest);
        }
        Ok(match api {
            WindowsApi32::GetLastError => (WindowsOutcome32::Return(self.last_error), self),
            WindowsApi32::SetLastError => (
                WindowsOutcome32::Return(frame.state().registers[Register32::Eax.index()]),
                Self {
                    last_error: frame.arguments()[0],
                },
            ),
            WindowsApi32::ExitProcess => {
                (WindowsOutcome32::ExitProcess(frame.arguments()[0]), self)
            }
            WindowsApi32::GetModuleHandleA => {
                if frame.arguments()[0] != 0 {
                    return Err(FrameError::InvalidRequest);
                }
                let base = context.main_image_base.ok_or(FrameError::InvalidRequest)?;
                (WindowsOutcome32::Return(base.0), self)
            }
        })
    }
}
