use super::{buffers, startup::ProcessStartup32};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::process::{Environment32, ProcessError32},
};
impl ProcessStartup32 {
    pub(super) fn environment_snapshot(
        &mut self,
        memory: &mut AddressSpace,
        environment: &Environment32,
    ) -> Result<u32, ProcessError32> {
        let bytes = environment
            .block()
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let mut arena = self.prepare_arena()?;
        let address = arena.allocate(bytes.len(), 2)?;
        self.environment_blocks
            .try_reserve(1)
            .map_err(|_| ProcessError32::Allocation)?;
        buffers::write(memory, address, &bytes)?;
        self.commit_arena(arena);
        self.environment_blocks.push((address, bytes.len()));
        Ok(address)
    }
    pub(super) fn free_environment_snapshot(&mut self, address: u32) -> Result<(), ProcessError32> {
        let index = self
            .environment_blocks
            .iter()
            .position(|(pointer, _)| *pointer == address)
            .ok_or(ProcessError32::InvalidParameter)?;
        let mut arena = self.prepare_arena()?;
        let (pointer, length) = self.environment_blocks[index];
        arena.release(pointer, length)?;
        self.environment_blocks.remove(index);
        self.commit_arena(arena);
        Ok(())
    }
    pub(super) fn replace_environment(
        &mut self,
        memory: &mut AddressSpace,
        environment: &Environment32,
    ) -> Result<(), ProcessError32> {
        let bytes = environment
            .block()
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let mut arena = self.prepare_arena()?;
        let address = arena.allocate(bytes.len(), 2)?;
        arena.release(self.current_environment.0, self.current_environment.1)?;
        memory
            .write_spans(&[
                (GuestAddress(address), &bytes),
                (GuestAddress(self.peb + 0x148), &address.to_le_bytes()),
            ])
            .map_err(|_| ProcessError32::NoAccess)?;
        self.commit_arena(arena);
        self.current_environment = (address, bytes.len());
        Ok(())
    }
}
