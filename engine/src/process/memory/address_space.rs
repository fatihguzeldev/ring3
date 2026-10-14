use crate::process::{EngineInstance, HostError};
use crate::{
    abi::arena::{TRANSFER_OFFSET, TRANSFER_SIZE},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

impl EngineInstance {
    pub fn memory(&self) -> Result<&AddressSpace, HostError> {
        let memory = self.retained_memory()?;
        if self.exit_code.is_some() {
            return Err(HostError::ProcessExited);
        }
        Ok(memory)
    }

    pub(in crate::process) fn retained_memory(&self) -> Result<&AddressSpace, HostError> {
        self.memory.as_ref().ok_or(HostError::Closed)
    }

    pub fn map(&mut self, address: u32, pages: u32, bits: u32) -> Result<(), HostError> {
        self.memory()?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let permissions = permissions(bits)?;
        let range = PageRange::new(GuestAddress(address), pages).map_err(HostError::Memory)?;
        memory
            .map_zeroed(range, permissions)
            .map_err(HostError::Memory)
    }

    pub fn protect(&mut self, address: u32, pages: u32, bits: u32) -> Result<(), HostError> {
        self.memory()?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let permissions = permissions(bits)?;
        let range = PageRange::new(GuestAddress(address), pages).map_err(HostError::Memory)?;
        memory
            .protect(range, permissions)
            .map_err(HostError::Memory)
    }

    pub fn unmap(&mut self, address: u32, pages: u32) -> Result<(), HostError> {
        self.memory()?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let range = PageRange::new(GuestAddress(address), pages).map_err(HostError::Memory)?;
        memory.unmap(range).map_err(HostError::Memory)?;
        self.revoke_virtual_allocations(range);
        Ok(())
    }

    pub fn upload(&mut self, address: u32, length: u32) -> Result<(), HostError> {
        self.memory()?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        if length as usize > TRANSFER_SIZE {
            return Err(HostError::InvalidRequest);
        }
        let input =
            &self.arena.as_ref().get_ref()[TRANSFER_OFFSET..TRANSFER_OFFSET + length as usize];
        memory
            .write(GuestAddress(address), input)
            .map_err(HostError::Memory)
    }
}

fn permissions(bits: u32) -> Result<Permissions, HostError> {
    u8::try_from(bits)
        .ok()
        .and_then(Permissions::from_bits)
        .ok_or(HostError::InvalidRequest)
}
