use super::*;
use crate::{
    memory::{Access, GuestAddress},
    process::{CallError, EngineInstance, HostError, calls::call::PendingOwner},
    windows::{CallFrame32, CallingConvention32, WindowsApi32, memory::MemoryApi32},
};

impl EngineInstance {
    pub(in crate::process) fn try_complete_memory_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let virtual_api = WindowsApi32::from_id(id).filter(|api| {
            matches!(
                api,
                WindowsApi32::VirtualAlloc
                    | WindowsApi32::VirtualFree
                    | WindowsApi32::VirtualProtect
                    | WindowsApi32::FlushInstructionCache
            )
        });
        let api = MemoryApi32::from_id(id);
        if api.is_none() && virtual_api.is_none() {
            return Ok(false);
        }
        let words = api.map_or_else(
            || virtual_api.unwrap().stack_words(),
            MemoryApi32::stack_words,
        );
        self.validate_windows_provider_owner(owner)?;
        self.checked_pending_call(owner, token)?;
        if frame.convention() != CallingConvention32::Stdcall || frame.stack_words() != words {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        Self::prepare_completed_call(frame.complete(0))?;
        self.check_windows_thread_write()?;
        let thread = self.current_windows_thread()?;
        if matches!(
            api,
            Some(MemoryApi32::LocalUnlock | MemoryApi32::GlobalUnlock)
        ) && thread.last_error() != 0
            && self.module_teb_address().is_some()
        {
            self.memory()?
                .ensure_transaction_versions(1)
                .map_err(HostError::Memory)?;
        }
        let result = if let Some(api) = api {
            self.execute_memory_call(api, frame.arguments())
        } else {
            match virtual_api.unwrap() {
                WindowsApi32::VirtualAlloc => self.memory_allocate(frame.arguments()),
                WindowsApi32::VirtualFree => self.memory_free(frame.arguments()),
                WindowsApi32::VirtualProtect => self.memory_protect(frame.arguments()),
                WindowsApi32::FlushInstructionCache => {
                    self.flush_windows_instruction_cache(frame.arguments())
                }
                _ => unreachable!(),
            }
        };
        if result == Err(MemoryError32::HeapCorruption) {
            self.begin_windows_heap_corruption(owner, frame)?;
            return Ok(true);
        }
        if matches!(api, Some(MemoryApi32::HeapAlloc | MemoryApi32::HeapReAlloc))
            && self
                .windows_memory
                .heaps
                .generates_exceptions(frame.arguments()[0], frame.arguments()[1])
            && let Err(error) = result
            && let Some(code) = heap_exception_code(error)
        {
            let mut original = self.windows_context_state()?;
            original.state = frame.complete(0);
            let record = crate::windows::exceptions::ExceptionRecord32::new(
                code,
                0,
                original.state.eip,
                &[],
            )
            .map_err(HostError::Exception)?;
            self.begin_exception_delivery(record, original, owner, None, None, None)?;
            return Ok(true);
        }
        let (value, error) = match result {
            Ok(value) => (
                value,
                if matches!(
                    api,
                    Some(MemoryApi32::LocalUnlock | MemoryApi32::GlobalUnlock)
                ) && value == 0
                {
                    Some(0)
                } else {
                    None
                },
            ),
            Err(error) => {
                if error == MemoryError32::Exhausted {
                    return Err(HostError::Memory(
                        crate::memory::MemoryError::VersionExhausted,
                    ));
                }
                let value = match api {
                    Some(MemoryApi32::HeapSize) => u32::MAX,
                    Some(MemoryApi32::LocalFree | MemoryApi32::GlobalFree) => frame.arguments()[0],
                    Some(MemoryApi32::LocalFlags | MemoryApi32::GlobalFlags) => 0x8000,
                    _ => 0,
                };
                (
                    value,
                    if matches!(api, Some(MemoryApi32::HeapAlloc | MemoryApi32::HeapReAlloc)) {
                        None
                    } else {
                        Some(error.win32())
                    },
                )
            }
        };
        let prepared = Self::prepare_completed_call(frame.complete(value))?;
        self.publish_windows_thread(error.map_or(thread, |error| thread.with_last_error(error)))?;
        self.publish_prepared_call(prepared);
        Ok(true)
    }
    fn execute_memory_call(
        &mut self,
        api: MemoryApi32,
        args: &[u32],
    ) -> Result<u32, MemoryError32> {
        use MemoryApi32::*;
        match api {
            VirtualAllocEx | VirtualFreeEx | VirtualProtectEx | VirtualQueryEx => {
                self.validate_windows_process_handle(
                    args[0],
                    if api == VirtualQueryEx { 0x400 } else { 8 },
                )
                .map_err(|error| {
                    if error
                        == crate::windows::synchronization::SynchronizationError32::AccessDenied
                    {
                        MemoryError32::AccessDenied
                    } else {
                        MemoryError32::InvalidHandle
                    }
                })?;
                match api {
                    VirtualAllocEx => self.memory_allocate(&args[1..]),
                    VirtualFreeEx => self.memory_free(&args[1..]),
                    VirtualProtectEx => self.memory_protect(&args[1..]),
                    _ => self.memory_query(&args[1..]),
                }
            }
            VirtualQuery => self.memory_query(args),
            HeapSetInformation => self.set_heap_information(args),
            HeapQueryInformation => self.query_heap_information(args),
            InitializeSListHead => self.initialize_slist_head(args[0]),
            GetProcessHeap => self.ensure_process_heap().map_err(|error| match error {
                HostError::WindowsMemory(error) => error,
                _ => MemoryError32::Allocation,
            }),
            HeapCreate => {
                let external = self
                    .guest_address_reservations()
                    .map_err(|_| MemoryError32::Allocation)?;
                self.windows_memory.heaps.create(
                    &mut self.windows_memory.virtual_memory,
                    self.memory.as_mut().unwrap(),
                    &external,
                    args[0],
                    args[1],
                    args[2],
                )
            }
            HeapDestroy => {
                self.windows_memory.heaps.destroy(
                    &mut self.windows_memory.virtual_memory,
                    self.memory.as_mut().unwrap(),
                    args[0],
                )?;
                Ok(1)
            }
            HeapAlloc => self.windows_memory.heaps.allocate(
                &mut self.windows_memory.virtual_memory,
                self.memory.as_mut().unwrap(),
                args[0],
                args[1],
                args[2],
            ),
            HeapReAlloc => self.windows_memory.heaps.reallocate(
                &mut self.windows_memory.virtual_memory,
                self.memory.as_mut().unwrap(),
                args[0],
                args[1],
                args[2],
                args[3],
            ),
            HeapFree => {
                self.windows_memory.heaps.free(args[0], args[1], args[2])?;
                Ok(1)
            }
            HeapSize => self.windows_memory.heaps.size(args[0], args[1], args[2]),
            HeapValidate => Ok(u32::from(
                self.windows_memory
                    .heaps
                    .validate(args[0], args[1], args[2])?,
            )),
            HeapLock | HeapUnlock => {
                self.windows_memory.heaps.lock(args[0], api == HeapUnlock)?;
                Ok(1)
            }
            HeapCompact => self.windows_memory.heaps.compact(
                &self.windows_memory.virtual_memory,
                self.memory.as_ref().unwrap(),
                args[0],
                args[1],
            ),
            HeapWalk => {
                let memory = self.memory.as_ref().unwrap();
                memory.check_span(GuestAddress(args[1]), 28, Access::Write)?;
                let mut bytes = [0; 4];
                memory.read(GuestAddress(args[1]), &mut bytes)?;
                let entry = self.windows_memory.heaps.walk(
                    &self.windows_memory.virtual_memory,
                    memory,
                    args[0],
                    u32::from_le_bytes(bytes),
                )?;
                self.memory
                    .as_mut()
                    .unwrap()
                    .write(GuestAddress(args[1]), &entry)?;
                Ok(1)
            }
            GetProcessHeaps => {
                let count = self.windows_memory.heaps.handles().count().max(1);
                self.memory.as_ref().unwrap().check_span(
                    GuestAddress(args[1]),
                    count.min(args[0] as usize) * 4,
                    Access::Write,
                )?;
                self.ensure_process_heap()
                    .map_err(|_| MemoryError32::Allocation)?;
                let count = self.windows_memory.heaps.handles().count();
                let length = count.min(args[0] as usize);
                let mut bytes = Vec::new();
                bytes
                    .try_reserve_exact(length * 4)
                    .map_err(|_| MemoryError32::Allocation)?;
                for handle in self.windows_memory.heaps.handles().take(length) {
                    bytes.extend_from_slice(&handle.to_le_bytes());
                }
                self.memory
                    .as_mut()
                    .unwrap()
                    .write(GuestAddress(args[1]), &bytes)?;
                Ok(count as u32)
            }
            LocalAlloc | GlobalAlloc => {
                let external = self
                    .guest_address_reservations()
                    .map_err(|_| MemoryError32::Allocation)?;
                self.windows_memory.heaps.local_allocate(
                    &mut self.windows_memory.virtual_memory,
                    self.memory.as_mut().unwrap(),
                    &external,
                    args[0],
                    args[1],
                )
            }
            LocalReAlloc | GlobalReAlloc => self.windows_memory.heaps.local_reallocate(
                &mut self.windows_memory.virtual_memory,
                self.memory.as_mut().unwrap(),
                args[0],
                args[1],
                args[2],
            ),
            LocalFree | GlobalFree => {
                self.windows_memory.heaps.local_free(args[0])?;
                Ok(0)
            }
            LocalLock | GlobalLock => self.windows_memory.heaps.local_lock(args[0]),
            LocalUnlock | GlobalUnlock => Ok(u32::from(
                self.windows_memory
                    .heaps
                    .local_unlock(args[0], api == GlobalUnlock)?,
            )),
            LocalSize => self.windows_memory.heaps.local_size(args[0]),
            GlobalSize => self
                .windows_memory
                .heaps
                .local_size(args[0])
                .map_err(|error| match error {
                    MemoryError32::InvalidParameter => MemoryError32::InvalidHandle,
                    error => error,
                }),
            LocalFlags | GlobalFlags => self.windows_memory.heaps.local_flags(args[0]),
            LocalHandle | GlobalHandle => self.windows_memory.heaps.local_handle(args[0]),
        }
    }
}

fn heap_exception_code(error: MemoryError32) -> Option<u32> {
    match error {
        MemoryError32::Capacity | MemoryError32::Allocation => Some(0xc0000017),
        MemoryError32::InvalidParameter
        | MemoryError32::InvalidHandle
        | MemoryError32::NoAccess => Some(0xc0000005),
        _ => None,
    }
}

impl EngineInstance {
    fn flush_windows_instruction_cache(&self, args: &[u32]) -> Result<u32, MemoryError32> {
        self.validate_windows_process_handle(args[0], 0)
            .map_err(|error| {
                if error == crate::windows::synchronization::SynchronizationError32::AccessDenied {
                    MemoryError32::AccessDenied
                } else {
                    MemoryError32::InvalidHandle
                }
            })?;
        if args[1] != 0 && u64::from(args[1]) + u64::from(args[2]) > 1 << 32 {
            return Err(MemoryError32::InvalidParameter);
        }
        // stores already invalidate translated code through the canonical page versions.
        Ok(1)
    }
}
