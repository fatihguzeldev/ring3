use super::{CallFrame32, FrameError, WindowsApi32};
use crate::{
    cpu::x86::Register32,
    memory::{GuestAddress, PAGE_SIZE, PageRange, Permissions},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WindowsOutcome32 {
    Return(u32),
    ExitProcess(u32),
    Allocate {
        size: u32,
        permissions: Permissions,
    },
    Release {
        address: u32,
    },
    Protect {
        range: PageRange,
        permissions: Permissions,
        old_output: GuestAddress,
    },
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
    pub(crate) fn last_error(self) -> u32 {
        self.last_error
    }

    pub(crate) fn with_last_error(self, last_error: u32) -> Self {
        Self { last_error }
    }

    pub(crate) fn allocation_failed(mut self) -> Self {
        self.last_error = 8;
        self
    }

    pub(crate) fn release_failed(mut self) -> Self {
        self.last_error = 487;
        self
    }

    pub(crate) fn protection_failed(mut self, error: u32) -> Self {
        self.last_error = error;
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
            WindowsApi32::LoadLibraryA
            | WindowsApi32::LoadLibraryW
            | WindowsApi32::LoadLibraryExA
            | WindowsApi32::LoadLibraryExW
            | WindowsApi32::GetProcAddress
            | WindowsApi32::FreeLibrary
            | WindowsApi32::GetModuleHandleW => return Err(FrameError::InvalidRequest),
            WindowsApi32::GetLastError => (WindowsOutcome32::Return(self.last_error), self),
            WindowsApi32::GetCurrentProcess => (WindowsOutcome32::Return(u32::MAX), self),
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
                if *address != 0 || !(1..=65_536).contains(size) || *allocation_type != 0x3000 {
                    return Err(FrameError::InvalidRequest);
                }
                let permissions = match *protection {
                    1 => Permissions::NONE,
                    2 => Permissions::READ,
                    4 => Permissions::READ_WRITE,
                    0x10 => Permissions::EXECUTE,
                    0x20 => Permissions::READ_EXECUTE,
                    0x40 => Permissions::ALL,
                    _ => return Err(FrameError::InvalidRequest),
                };
                (
                    WindowsOutcome32::Allocate {
                        size: *size,
                        permissions,
                    },
                    self,
                )
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
            WindowsApi32::VirtualProtect => {
                let [address, size, protection, old_output] = frame.arguments() else {
                    return Err(FrameError::InvalidRequest);
                };
                let end = u64::from(*address) + u64::from(*size);
                if !(1..=65_536).contains(size) || end > 1 << 32 {
                    return Err(FrameError::InvalidRequest);
                }
                let permissions = match *protection {
                    1 => Permissions::NONE,
                    2 => Permissions::READ,
                    4 => Permissions::READ_WRITE,
                    0x10 => Permissions::EXECUTE,
                    0x20 => Permissions::READ_EXECUTE,
                    0x40 => Permissions::ALL,
                    _ => return Err(FrameError::InvalidRequest),
                };
                let first = *address / PAGE_SIZE;
                let count = ((end - 1) / u64::from(PAGE_SIZE) - u64::from(first) + 1) as u32;
                let range = PageRange::new(GuestAddress(first * PAGE_SIZE), count)
                    .map_err(|_| FrameError::InvalidRequest)?;
                let target_end = u64::from(first + count) * u64::from(PAGE_SIZE);
                if *old_output != 0
                    && u64::from(*old_output) < target_end
                    && u64::from(first) * u64::from(PAGE_SIZE) < u64::from(*old_output) + 4
                {
                    return Err(FrameError::InvalidRequest);
                }
                (
                    WindowsOutcome32::Protect {
                        range,
                        permissions,
                        old_output: GuestAddress(*old_output),
                    },
                    self,
                )
            }
            WindowsApi32::FlushInstructionCache => {
                let [process, address, size] = frame.arguments() else {
                    return Err(FrameError::InvalidRequest);
                };
                if *process != u32::MAX
                    || (*address != 0 && u64::from(*address) + u64::from(*size) > 1 << 32)
                {
                    return Err(FrameError::InvalidRequest);
                }
                (WindowsOutcome32::Return(1), self)
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
        windows::CallingConvention32,
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
            for (protection, permissions) in [
                (1, Permissions::NONE),
                (2, Permissions::READ),
                (4, Permissions::READ_WRITE),
                (0x10, Permissions::EXECUTE),
                (0x20, Permissions::READ_EXECUTE),
                (0x40, Permissions::ALL),
            ] {
                let call = frame(
                    [0, size, 0x3000, protection],
                    CallingConvention32::Stdcall,
                    4,
                );
                let (outcome, next) = thread
                    .prepare(
                        WindowsApi32::VirtualAlloc,
                        &call,
                        ProcessContext32::default(),
                    )
                    .unwrap();
                assert_eq!(outcome, WindowsOutcome32::Allocate { size, permissions });
                assert_eq!(next.last_error, thread.last_error);
            }
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
            [0, 1, 0x3000, 0x140],
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
        for protection in [
            3,
            8,
            0x14,
            0x60,
            0x80,
            0x104,
            0x204,
            0x404,
            0x4000_0040,
            u32::MAX,
        ] {
            let call = frame([0, 1, 0x3000, protection], CallingConvention32::Stdcall, 4);
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

    #[test]
    fn virtual_protect_prepares_rounded_ranges_and_defers_output_access() {
        let thread = ThreadState32 {
            last_error: 0xf123_4567,
        };
        for (protection, permissions) in [
            (1, Permissions::NONE),
            (2, Permissions::READ),
            (4, Permissions::READ_WRITE),
            (0x10, Permissions::EXECUTE),
            (0x20, Permissions::READ_EXECUTE),
            (0x40, Permissions::ALL),
        ] {
            for output in [0, 0x8100, u32::MAX - 1] {
                let call = frame(
                    [0x1000_0003, 4094, protection, output],
                    CallingConvention32::Stdcall,
                    4,
                );
                let (outcome, next) = thread
                    .prepare(
                        WindowsApi32::VirtualProtect,
                        &call,
                        ProcessContext32::default(),
                    )
                    .unwrap();
                assert_eq!(
                    outcome,
                    WindowsOutcome32::Protect {
                        range: PageRange::new(GuestAddress(0x1000_0000), 2).unwrap(),
                        permissions,
                        old_output: GuestAddress(output),
                    }
                );
                assert_eq!(next.last_error, thread.last_error);
            }
        }
        assert_eq!(thread.protection_failed(487).last_error, 487);
        assert_eq!(thread.protection_failed(998).last_error, 998);
    }

    #[test]
    fn virtual_protect_rejects_shapes_and_any_rounded_page_output_overlap() {
        let thread = ThreadState32::default();
        for arguments in [
            [0x1000_0000, 0, 4, 0x8100],
            [0x1000_0000, 65_537, 4, 0x8100],
            [u32::MAX, 2, 4, 0x8100],
            [0x1000_0000, 1, 0x140, 0x8100],
            [0x1000_007f, 1, 4, 0x1000_0000],
            [0x1000_007f, 1, 4, 0x1000_0fff],
            [0x1000_007f, 1, 4, 0x0fff_fffd],
        ] {
            let call = frame(arguments, CallingConvention32::Stdcall, 4);
            assert_eq!(
                thread
                    .prepare(
                        WindowsApi32::VirtualProtect,
                        &call,
                        ProcessContext32::default()
                    )
                    .map(|_| ()),
                Err(FrameError::InvalidRequest)
            );
        }
        for (convention, count) in [
            (CallingConvention32::Cdecl, 4),
            (CallingConvention32::Thiscall, 4),
            (CallingConvention32::Stdcall, 3),
            (CallingConvention32::Stdcall, 5),
        ] {
            let call = frame([0x1000_0000, 1, 4, 0x8100], convention, count);
            assert_eq!(
                thread
                    .prepare(
                        WindowsApi32::VirtualProtect,
                        &call,
                        ProcessContext32::default()
                    )
                    .map(|_| ()),
                Err(FrameError::InvalidRequest)
            );
        }
    }
}
