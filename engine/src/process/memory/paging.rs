use crate::{
    cpu::x86::{
        CpuPagingError32, PagingAccess32, inspect_cpu_paging_access32, read_cpu_paging_access32,
    },
    memory::{GuestAddress, MemoryError, PagingBacking32},
    process::EngineInstance,
};

mod port;
#[cfg(any(target_arch = "wasm32", test))]
mod request;

impl EngineInstance {
    pub fn cr2(&self) -> Option<GuestAddress> {
        self.cr2
    }

    pub(in crate::process) fn read_cpu_paging32(
        &mut self,
        address: GuestAddress,
        output: &mut [u8],
        access: PagingAccess32,
    ) -> Result<(), CpuPagingError32> {
        u32::try_from(output.len())
            .map_err(|_| CpuPagingError32::Memory(MemoryError::InvalidRange))?;
        #[cfg(any(target_arch = "wasm32", test))]
        self.clear_certified_page_fault32();
        let result = read_cpu_paging_access32(
            self.memory
                .as_ref()
                .expect("live process guard precedes cpu read"),
            address,
            output,
            access,
        );
        if let Err(error) = result {
            self.record_cpu_paging_error32(error);
        }
        result
    }

    pub(in crate::process) fn preflight_cpu_paging32(
        &mut self,
        address: GuestAddress,
        length: u32,
        access: PagingAccess32,
    ) -> Result<(), CpuPagingError32> {
        let backing = match access {
            PagingAccess32::Read => PagingBacking32::Read,
            PagingAccess32::Write => PagingBacking32::Write,
            PagingAccess32::Execute => PagingBacking32::Execute,
        };
        self.preflight_cpu_paging_backing32(address, length, access, backing)
    }

    pub(in crate::process) fn preflight_cpu_read_modify32(
        &mut self,
        address: GuestAddress,
        length: u32,
    ) -> Result<(), CpuPagingError32> {
        self.preflight_cpu_paging_backing32(
            address,
            length,
            PagingAccess32::Write,
            PagingBacking32::Read,
        )
    }

    pub(in crate::process) fn preflight_cpu_modify32(
        &mut self,
        address: GuestAddress,
        length: u32,
    ) -> Result<(), CpuPagingError32> {
        self.preflight_cpu_paging_backing32(
            address,
            length,
            PagingAccess32::Write,
            PagingBacking32::Modify,
        )
    }

    #[cfg(test)]
    pub(in crate::process) fn inspect_cpu_paging32(
        &self,
        address: GuestAddress,
        length: u32,
        access: PagingAccess32,
    ) -> Result<(), CpuPagingError32> {
        let backing = match access {
            PagingAccess32::Read => PagingBacking32::Read,
            PagingAccess32::Write => PagingBacking32::Write,
            PagingAccess32::Execute => PagingBacking32::Execute,
        };
        self.inspect_cpu_paging_backing32(address, length, access, backing)
    }

    pub(in crate::process) fn record_cpu_paging_error32(&mut self, error: CpuPagingError32) {
        #[cfg(any(target_arch = "wasm32", test))]
        self.clear_certified_page_fault32();
        if let CpuPagingError32::PageFault(fault) = error {
            // one bounded cpu helper records far fewer than 2^64 faults, so wrapping
            // still changes the before/after sequence, including identical repeated faults.
            self.cpu_page_fault_sequence = self.cpu_page_fault_sequence.wrapping_add(1);
            self.cr2 = Some(fault.linear());
            #[cfg(any(target_arch = "wasm32", test))]
            self.observe_certified_page_fault32(fault);
        }
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub(in crate::process) const fn cpu_page_fault_sequence32(&self) -> u64 {
        self.cpu_page_fault_sequence
    }

    fn preflight_cpu_paging_backing32(
        &mut self,
        address: GuestAddress,
        length: u32,
        access: PagingAccess32,
        backing: PagingBacking32,
    ) -> Result<(), CpuPagingError32> {
        #[cfg(any(target_arch = "wasm32", test))]
        self.clear_certified_page_fault32();
        let result = self.inspect_cpu_paging_backing32(address, length, access, backing);
        if let Err(error) = result {
            self.record_cpu_paging_error32(error);
        }
        result
    }

    fn inspect_cpu_paging_backing32(
        &self,
        address: GuestAddress,
        length: u32,
        access: PagingAccess32,
        backing: PagingBacking32,
    ) -> Result<(), CpuPagingError32> {
        // all callers already passed their process lifetime/owner guard.
        let memory = self
            .memory
            .as_ref()
            .expect("live process guard precedes cpu preflight");
        inspect_cpu_paging_access32(memory, address, length, access, backing)
    }

    pub(in crate::process) fn preflight_cpu_paging_masked32(
        &mut self,
        address: GuestAddress,
        length: u32,
        mask: u32,
        access: PagingAccess32,
    ) -> Result<(), CpuPagingError32> {
        if length == 0 || length > 32 || length < 32 && mask >> length != 0 {
            return Err(CpuPagingError32::Memory(MemoryError::InvalidRange));
        }
        #[cfg(any(target_arch = "wasm32", test))]
        self.clear_certified_page_fault32();
        for offset in 0..length {
            if mask & (1 << offset) != 0 {
                self.preflight_cpu_paging32(
                    GuestAddress(address.0.wrapping_add(offset)),
                    1,
                    access,
                )?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
