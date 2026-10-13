#[cfg(any(target_arch = "wasm32", test))]
use crate::abi::{
    arena::TRANSFER_OFFSET,
    x86::{SYSTEM_REGISTER_BANK_SIZE, decode_system_register_bank},
};
use crate::{
    cpu::x86::UserSystemRegisterBank32,
    process::{CallError, EngineInstance, HostError},
};

impl EngineInstance {
    fn system_register_configuration_guard(&self) -> Result<(), HostError> {
        self.memory()?;
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        if self.system_register_bank.is_some()
            || self.generation != 0
            || self.artifact.is_some()
            || self.resident.is_some()
            || self.image_started
        {
            return Err(HostError::InvalidRequest);
        }
        Ok(())
    }

    /// Configure readback authority once before publishing executable material.
    /// Images remain separate from sparse descriptors and are never dereferenced here.
    pub fn configure_system_register_bank(
        &mut self,
        bank: UserSystemRegisterBank32,
    ) -> Result<(), HostError> {
        self.system_register_configuration_guard()?;
        if !bank.is_valid() {
            return Err(HostError::InvalidRequest);
        }
        self.system_register_bank = Some(bank);
        Ok(())
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub(in crate::process) fn configure_system_register_bank_request(
        &mut self,
    ) -> Result<(), HostError> {
        self.system_register_configuration_guard()?;
        let bank = decode_system_register_bank(
            &self.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + SYSTEM_REGISTER_BANK_SIZE],
        )
        .map_err(|_| HostError::InvalidRequest)?;
        self.system_register_bank = Some(bank);
        Ok(())
    }
}

#[cfg(test)]
#[path = "system_registers/tests.rs"]
mod tests;
