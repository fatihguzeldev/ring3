use crate::{
    process::{EngineInstance, HostError},
    windows::memory::{GlobalMemory32, MemoryError32, PreparedGlobalRetirement32},
};

impl EngineInstance {
    pub(in crate::process) fn windows_global_memory(
        &self,
        handle: u32,
    ) -> Result<GlobalMemory32, HostError> {
        self.windows_memory
            .heaps
            .global_memory(handle)
            .map_err(HostError::WindowsMemory)
    }

    pub(in crate::process) fn check_windows_global_memory(
        &self,
        memory: GlobalMemory32,
    ) -> Result<(), HostError> {
        self.windows_memory
            .heaps
            .check_global_memory(memory)
            .map_err(HostError::WindowsMemory)
    }

    pub(in crate::process) fn commit_windows_global_transfer(&mut self, memory: GlobalMemory32) {
        self.windows_memory.heaps.commit_global_transfer(memory);
    }

    pub(in crate::process) fn allocate_windows_transferred_global(
        &mut self,
        bytes: &[u8],
    ) -> Result<GlobalMemory32, HostError> {
        self.ensure_process_heap()?;
        let size = u32::try_from(bytes.len())
            .map_err(|_| HostError::WindowsMemory(MemoryError32::Capacity))?;
        let prepared: crate::windows::memory::PreparedGlobalMemory32 = self
            .windows_memory
            .heaps
            .prepare_transferred_global(self.memory.as_ref().ok_or(HostError::Closed)?, size)
            .map_err(HostError::WindowsMemory)?;
        self.windows_memory
            .heaps
            .commit_transferred_global(
                &mut self.windows_memory.virtual_memory,
                self.memory.as_mut().unwrap(),
                prepared,
                bytes,
            )
            .map_err(HostError::WindowsMemory)
    }

    pub(in crate::process) fn prepare_windows_global_retirement(
        &self,
        owned: &[GlobalMemory32],
    ) -> Result<Option<PreparedGlobalRetirement32>, HostError> {
        if owned.is_empty() {
            return Ok(None);
        }
        self.windows_memory
            .heaps
            .prepare_global_retirement(owned)
            .map(Some)
            .map_err(HostError::WindowsMemory)
    }

    pub(in crate::process) fn commit_windows_global_retirement(
        &mut self,
        prepared: Option<PreparedGlobalRetirement32>,
    ) {
        if let Some(prepared) = prepared {
            self.windows_memory.heaps.commit_global_retirement(prepared);
        }
    }
}
