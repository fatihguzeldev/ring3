use super::{CallFrame32, CallingConvention32, FrameError};
use crate::{cpu::x86::Register32, memory::GuestAddress};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum WindowsApi32 {
    GetLastError = 0x0001_0001,
    SetLastError = 0x0001_0002,
    ExitProcess = 0x0001_0003,
    GetModuleHandleA = 0x0001_0004,
    VirtualAlloc = 0x0001_0005,
    VirtualFree = 0x0001_0006,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WindowsOutcome32 {
    Return(u32),
    ExitProcess(u32),
    Allocate { size: u32 },
    Release { address: u32 },
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
            "VirtualAlloc" => Some(Self::VirtualAlloc),
            "VirtualFree" => Some(Self::VirtualFree),
            _ => None,
        }
    }

    pub fn from_id(id: u32) -> Option<Self> {
        match id {
            0x0001_0001 => Some(Self::GetLastError),
            0x0001_0002 => Some(Self::SetLastError),
            0x0001_0003 => Some(Self::ExitProcess),
            0x0001_0004 => Some(Self::GetModuleHandleA),
            0x0001_0005 => Some(Self::VirtualAlloc),
            0x0001_0006 => Some(Self::VirtualFree),
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
            Self::VirtualAlloc => 4,
            Self::VirtualFree => 3,
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
    pub(crate) fn allocation_failed(mut self) -> Self {
        self.last_error = 8;
        self
    }

    pub(crate) fn release_failed(mut self) -> Self {
        self.last_error = 487;
        self
    }

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
            WindowsApi32::VirtualAlloc => {
                let [address, size, allocation_type, protection] = frame.arguments() else {
                    return Err(FrameError::InvalidRequest);
                };
                if *address != 0
                    || !(1..=65_536).contains(size)
                    || *allocation_type != 0x3000
                    || *protection != 4
                {
                    return Err(FrameError::InvalidRequest);
                }
                (WindowsOutcome32::Allocate { size: *size }, self)
            }
            WindowsApi32::VirtualFree => {
                let [address, size, free_type] = frame.arguments() else {
                    return Err(FrameError::InvalidRequest);
                };
                if *size != 0 || *free_type != 0x8000 {
                    return Err(FrameError::InvalidRequest);
                }
                (WindowsOutcome32::Release { address: *address }, self)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cpu::x86::State32,
        memory::{AddressSpace, PageRange, Permissions},
    };

    fn frame(arguments: [u32; 4], convention: CallingConvention32, count: u32) -> CallFrame32 {
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x8000), 1).unwrap(),
                Permissions::READ_WRITE,
            )
            .unwrap();
        for (index, value) in [0x9000].into_iter().chain(arguments).enumerate() {
            memory
                .write(
                    GuestAddress(0x8080 + index as u32 * 4),
                    &value.to_le_bytes(),
                )
                .unwrap();
        }
        let mut state = State32::default();
        state.registers[Register32::Esp.index()] = 0x8080;
        CallFrame32::capture(&memory, state, convention, count).unwrap()
    }

    #[test]
    fn virtual_alloc_only_prepares_a_bounded_request_and_preserves_last_error() {
        let thread = ThreadState32 {
            last_error: 0xf123_4567,
        };
        for size in [1, 4096, 4097, 65_536] {
            let call = frame([0, size, 0x3000, 4], CallingConvention32::Stdcall, 4);
            let (outcome, next) = thread
                .prepare(
                    WindowsApi32::VirtualAlloc,
                    &call,
                    ProcessContext32::default(),
                )
                .unwrap();
            assert_eq!(outcome, WindowsOutcome32::Allocate { size });
            assert_eq!(next.last_error, thread.last_error);
        }
        assert_eq!(thread.allocation_failed().last_error, 8);
        assert_eq!(thread.last_error, 0xf123_4567);
    }

    #[test]
    fn virtual_alloc_rejects_each_unsupported_shape_without_changing_thread() {
        let thread = ThreadState32 {
            last_error: 0xf123_4567,
        };
        for arguments in [
            [1, 1, 0x3000, 4],
            [0, 0, 0x3000, 4],
            [0, 65_537, 0x3000, 4],
            [0, u32::MAX, 0x3000, 4],
            [0, 1, 0x1000, 4],
            [0, 1, 0x2000, 4],
            [0, 1, 0x3001, 4],
            [0, 1, 0x3000, 0],
            [0, 1, 0x3000, 0x40],
        ] {
            let call = frame(arguments, CallingConvention32::Stdcall, 4);
            assert!(matches!(
                thread.prepare(
                    WindowsApi32::VirtualAlloc,
                    &call,
                    ProcessContext32::default()
                ),
                Err(FrameError::InvalidRequest)
            ));
        }
        for (convention, count) in [
            (CallingConvention32::Cdecl, 4),
            (CallingConvention32::Thiscall, 4),
            (CallingConvention32::Stdcall, 3),
            (CallingConvention32::Stdcall, 5),
        ] {
            let call = frame([0, 1, 0x3000, 4], convention, count);
            assert!(matches!(
                thread.prepare(
                    WindowsApi32::VirtualAlloc,
                    &call,
                    ProcessContext32::default()
                ),
                Err(FrameError::InvalidRequest)
            ));
        }
        assert_eq!(thread.last_error, 0xf123_4567);
    }
}
