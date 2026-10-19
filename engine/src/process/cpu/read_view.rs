use crate::{
    memory::{AddressSpace, MAX_RESIDENT_PAGES},
    process::{EngineInstance, calls::call::PendingOwner, cpu::events::detection::DetectionSlot32},
};

// wasm32 addresses only; the registry keeps the containing instance in place until wasm death.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::process) struct CpuRead32View32 {
    active_epoch: u64,
    mappings: u32,
    permissions: u32,
    backing: u32,
    resident_pages: u32,
}

impl CpuRead32View32 {
    pub(in crate::process) fn for_memory(memory: &AddressSpace) -> Self {
        let (mappings, permissions, backing) = memory.cpu_read_arrays();
        Self {
            active_epoch: 0,
            mappings: mappings.as_ptr() as usize as u32,
            permissions: permissions.as_ptr() as usize as u32,
            backing: backing.as_ptr() as usize as u32,
            resident_pages: memory.capacity_pages(),
        }
    }
}

impl EngineInstance {
    fn cpu_read32_view_epoch(&self) -> u64 {
        let Some(memory) = self.memory.as_ref() else {
            return 0;
        };
        if !(1..=MAX_RESIDENT_PAGES).contains(&memory.capacity_pages()) {
            return 0;
        }
        match self.exception_detection.as_ref().map(|slot| &slot[0]) {
            Some(DetectionSlot32::Active {
                epoch,
                owner: PendingOwner::Resident(_) | PendingOwner::ResidentCallback { .. },
                page_fault: None,
                ..
            }) => *epoch,
            _ => 0,
        }
    }

    pub(in crate::process) fn revoke_cpu_read32_view(&mut self) {
        self.cpu_read32_view.active_epoch = 0;
    }

    pub(in crate::process) fn reset_cpu_read32_view(&mut self) {
        self.cpu_read32_view = self
            .memory
            .as_ref()
            .map(CpuRead32View32::for_memory)
            .unwrap_or_default();
    }

    pub(in crate::process) fn refresh_cpu_read32_view(&mut self) {
        self.cpu_read32_view.active_epoch = self.cpu_read32_view_epoch();
    }

    pub(in crate::process) fn cpu_read32_view_address(&self, epoch: u64) -> usize {
        if epoch == 0
            || self.cpu_read32_view.active_epoch != epoch
            || self.cpu_read32_view_epoch() != epoch
        {
            return 0;
        }
        &self.cpu_read32_view as *const CpuRead32View32 as usize
    }
}

#[cfg(test)]
pub(in crate::process) mod tests;
