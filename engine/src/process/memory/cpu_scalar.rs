use crate::{
    abi::{
        arena::HELPER_OFFSET,
        memory_helper::{CpuScalarOperation, HELPER_SIZE, encode_cpu_scalar_result},
    },
    memory::GuestAddress,
    process::{EngineInstance, HostError},
};

impl EngineInstance {
    pub fn cpu_read8(&mut self, address: u32) -> Result<(), HostError> {
        self.cpu_scalar_read::<1>(address)
    }

    pub fn cpu_read16(&mut self, address: u32) -> Result<(), HostError> {
        self.cpu_scalar_read::<2>(address)
    }

    pub fn cpu_read32(&mut self, address: u32) -> Result<(), HostError> {
        self.cpu_scalar_read::<4>(address)
    }

    fn cpu_scalar_read<const WIDTH: u32>(&mut self, address: u32) -> Result<(), HostError> {
        let width = WIDTH;
        // cpu helper lifetime admission is separate from diagnostic reads; DBT checks segments.
        let memory = self.memory()?;
        let mut bytes = [0; 4];
        let result = memory
            .cpu_read(GuestAddress(address), &mut bytes[..width as usize])
            .map(|()| u32::from_le_bytes(bytes));
        encode_cpu_scalar_result(
            GuestAddress(address),
            width,
            CpuScalarOperation::Read,
            result,
            &mut self.arena.as_mut().get_mut()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)?;
        Ok(())
    }
}
