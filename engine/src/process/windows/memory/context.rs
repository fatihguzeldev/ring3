use super::*;
use crate::process::{EngineInstance, HostError};

impl EngineInstance {
    pub(in crate::process) fn prepare_windows_runtime_retirement(
        &self,
        owned: &[(u32, u64)],
    ) -> Result<Option<crate::windows::memory::PreparedHeapRetirement32>, HostError> {
        if owned.is_empty() {
            return Ok(None);
        }
        let heap = self
            .process_heap()
            .ok_or(HostError::WindowsMemory(MemoryError32::InvalidHandle))?;
        self.windows_memory
            .heaps
            .prepare_retirement(heap, owned)
            .map(Some)
            .map_err(HostError::WindowsMemory)
    }

    pub(in crate::process) fn commit_windows_runtime_retirement_after_memory(
        &mut self,
        prepared: Option<crate::windows::memory::PreparedHeapRetirement32>,
    ) {
        if let Some(prepared) = prepared {
            self.windows_memory
                .heaps
                .commit_retirement_after_memory(prepared);
        }
    }

    pub(in crate::process) fn windows_runtime_allocation_identity(
        &self,
        pointer: u32,
    ) -> Result<u64, HostError> {
        let heap = self
            .process_heap()
            .ok_or(HostError::WindowsMemory(MemoryError32::InvalidHandle))?;
        self.windows_memory
            .heaps
            .allocation_identity(heap, pointer)
            .map_err(HostError::WindowsMemory)
    }

    pub(in crate::process) fn allocate_windows_runtime_initialized(
        &mut self,
        bytes: &[u8],
    ) -> Result<u32, HostError> {
        let heap = self.ensure_process_heap()?;
        self.windows_memory
            .heaps
            .allocate_initialized(
                &mut self.windows_memory.virtual_memory,
                self.memory.as_mut().unwrap(),
                heap,
                bytes,
            )
            .map_err(HostError::WindowsMemory)
    }
    pub(in crate::process) fn allocate_windows_runtime_bytes(
        &mut self,
        size: u32,
    ) -> Result<u32, HostError> {
        let heap = self.ensure_process_heap()?;
        self.windows_memory
            .heaps
            .allocate(
                &mut self.windows_memory.virtual_memory,
                self.memory.as_mut().unwrap(),
                heap,
                0,
                size,
            )
            .map_err(HostError::WindowsMemory)
    }
    pub(in crate::process) fn check_windows_runtime_bytes(
        &self,
        pointer: u32,
        size: u32,
    ) -> Result<(), HostError> {
        let heap = self
            .process_heap()
            .ok_or(HostError::WindowsMemory(MemoryError32::InvalidHandle))?;
        if self
            .windows_memory
            .heaps
            .size(heap, 0, pointer)
            .map_err(HostError::WindowsMemory)?
            < size
        {
            return Err(HostError::WindowsMemory(MemoryError32::InvalidParameter));
        }
        self.memory()?
            .check_span(
                crate::memory::GuestAddress(pointer),
                size as usize,
                crate::memory::Access::Write,
            )
            .map_err(HostError::Memory)
    }
    pub(in crate::process) fn free_windows_runtime_bytes(&mut self, pointer: u32) {
        self.release_windows_runtime_bytes(pointer)
            .expect("runtime allocation ownership was preflighted");
    }
    pub(in crate::process) fn release_windows_runtime_bytes(
        &mut self,
        pointer: u32,
    ) -> Result<(), HostError> {
        let heap = self.process_heap().unwrap();
        self.windows_memory
            .heaps
            .free(heap, 0, pointer)
            .map_err(HostError::WindowsMemory)
    }
    pub(in crate::process) fn resize_windows_runtime_bytes(
        &mut self,
        pointer: u32,
        size: u32,
    ) -> Result<u32, HostError> {
        let heap = self.ensure_process_heap()?;
        self.windows_memory
            .heaps
            .reallocate(
                &mut self.windows_memory.virtual_memory,
                self.memory.as_mut().unwrap(),
                heap,
                0,
                pointer,
                size,
            )
            .map_err(HostError::WindowsMemory)
    }
    pub fn configure_windows_memory(&mut self, limits: MemoryLimits32) -> Result<(), HostError> {
        self.memory()?;
        if !self.windows_memory.is_empty() || self.pending_call.is_some() || self.callback.is_some()
        {
            return Err(HostError::WindowsMemory(MemoryError32::Busy));
        }
        self.windows_memory =
            WindowsMemoryRuntime32::new(limits).map_err(HostError::WindowsMemory)?;
        Ok(())
    }
    pub fn process_heap(&self) -> Option<u32> {
        self.windows_memory.heaps.default_heap()
    }
    pub(in crate::process) fn ensure_process_heap(&mut self) -> Result<u32, HostError> {
        self.memory()?;
        let external = self.guest_address_reservations()?;
        self.windows_memory
            .heaps
            .ensure_default(
                &mut self.windows_memory.virtual_memory,
                self.memory.as_mut().unwrap(),
                &external,
            )
            .map_err(HostError::WindowsMemory)
    }
}
