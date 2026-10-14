#[cfg(any(target_arch = "wasm32", test))]
use crate::abi::{
    arena::TRANSFER_OFFSET,
    x86::{INTERRUPT_DESCRIPTOR_TABLE_SIZE, decode_interrupt_descriptor_table},
};
use crate::{
    cpu::x86::InterruptDescriptorTable32,
    process::{CallError, EngineInstance, HostError},
};

impl EngineInstance {
    fn interrupt_configuration_guard(&self) -> Result<(), HostError> {
        self.memory()?;
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        if self.interrupt_descriptors.is_some()
            || self.generation != 0
            || self.artifact.is_some()
            || self.resident.is_some()
            || self.image_started
        {
            return Err(HostError::InvalidRequest);
        }
        Ok(())
    }

    /// configure exact interrupt authority once before executable material is published.
    pub fn configure_interrupt_descriptor_table(
        &mut self,
        table: InterruptDescriptorTable32,
    ) -> Result<(), HostError> {
        self.interrupt_configuration_guard()?;
        self.interrupt_descriptors = Some(table);
        Ok(())
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub(in crate::process) fn configure_interrupt_descriptor_table_request(
        &mut self,
    ) -> Result<(), HostError> {
        self.interrupt_configuration_guard()?;
        let table = decode_interrupt_descriptor_table(
            &self.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + INTERRUPT_DESCRIPTOR_TABLE_SIZE],
        )
        .map_err(|_| HostError::InvalidRequest)?;
        self.interrupt_descriptors = Some(table);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
