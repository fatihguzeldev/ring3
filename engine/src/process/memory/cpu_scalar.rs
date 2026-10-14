use crate::{
    abi::{
        arena::HELPER_OFFSET,
        memory_helper::{CpuScalarOperation, HELPER_SIZE, encode_cpu_paging_scalar_result},
    },
    cpu::x86::{CpuPagingError32, PagingAccess32},
    memory::GuestAddress,
    process::{EngineInstance, HostError},
};

impl EngineInstance {
    pub fn cpu_read8(&mut self, address: u32) -> Result<(), HostError> {
        self.cpu_scalar_read::<1>(address, false)
    }

    pub fn cpu_read16(&mut self, address: u32) -> Result<(), HostError> {
        self.cpu_scalar_read::<2>(address, false)
    }

    pub fn cpu_read32(&mut self, address: u32) -> Result<(), HostError> {
        self.cpu_scalar_read::<4>(address, false)
    }

    pub fn cpu_read_modify8(&mut self, address: u32) -> Result<(), HostError> {
        self.cpu_scalar_read::<1>(address, true)
    }
    pub fn cpu_read_modify16(&mut self, address: u32) -> Result<(), HostError> {
        self.cpu_scalar_read::<2>(address, true)
    }
    pub fn cpu_read_modify32(&mut self, address: u32) -> Result<(), HostError> {
        self.cpu_scalar_read::<4>(address, true)
    }

    fn cpu_scalar_read<const WIDTH: u32>(
        &mut self,
        address: u32,
        modify: bool,
    ) -> Result<(), HostError> {
        let width = WIDTH;
        // cpu helper lifetime admission is separate from diagnostic reads; DBT checks segments.
        self.memory()?;
        let at = GuestAddress(address);
        let preflight = if modify {
            self.preflight_cpu_read_modify32(at, width)
        } else {
            self.preflight_cpu_paging32(at, width, PagingAccess32::Read)
        };
        let mut bytes = [0; 4];
        let result = preflight.and_then(|()| {
            self.memory
                .as_ref()
                .unwrap()
                .cpu_read(at, &mut bytes[..width as usize])
                .map(|()| u32::from_le_bytes(bytes))
                .map_err(CpuPagingError32::Memory)
        });
        encode_cpu_paging_scalar_result(
            GuestAddress(address),
            width,
            if modify {
                CpuScalarOperation::Modify
            } else {
                CpuScalarOperation::Read
            },
            result,
            &mut self.arena.as_mut().get_mut()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)?;
        Ok(())
    }
}
