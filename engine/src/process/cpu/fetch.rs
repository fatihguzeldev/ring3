use crate::{
    abi::{
        arena::HELPER_OFFSET,
        cpu_helper::{CpuMemoryError, encode_cpu_fetch_result},
        memory_helper::HELPER_SIZE,
    },
    cpu::x86::PagingAccess32,
    memory::{Access, GuestAddress},
    process::{EngineInstance, HostError},
};

impl EngineInstance {
    pub fn cpu_fetch_fault(&mut self, pc: u32, attempted_length: u32) -> Result<(), HostError> {
        self.guard_material(self.key, self.generation())?;
        self.fetch_fault_bound(pc, attempted_length)
    }

    pub fn cpu_fetch_fault_resident(
        &mut self,
        key: u64,
        id: u64,
        pc: u32,
        attempted_length: u32,
    ) -> Result<(), HostError> {
        self.guard_material_resident(key, id)?;
        self.fetch_fault_bound(pc, attempted_length)
    }

    fn fetch_fault_bound(&mut self, pc: u32, attempted_length: u32) -> Result<(), HostError> {
        if !(1..=15).contains(&attempted_length) {
            return Err(HostError::InvalidRequest);
        }
        let cs = self.code_fetch_context()?;
        #[cfg(any(target_arch = "wasm32", test))]
        self.clear_certified_page_fault32();
        let result = if !cs.permits(GuestAddress(pc), attempted_length as u8) {
            Err(CpuMemoryError::GeneralProtection(0))
        } else {
            let linear = cs.linear_address(GuestAddress(pc));
            self.preflight_cpu_paging32(linear, attempted_length, PagingAccess32::Execute)
                .map_err(CpuMemoryError::from)
                .and_then(|()| {
                    self.memory
                        .as_ref()
                        .unwrap()
                        .cpu_check_span(linear, attempted_length as usize, Access::Execute)
                        .map_err(CpuMemoryError::Memory)
                })
        };
        encode_cpu_fetch_result(
            cs.linear_address(GuestAddress(pc)),
            attempted_length,
            result,
            &mut self.arena.as_mut().get_mut()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)
    }
}

#[cfg(test)]
mod tests;
