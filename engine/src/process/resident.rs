#![forbid(unsafe_code)]

use super::{CallError, EngineInstance, HostError};
use crate::{
    abi::arena::TRANSFER_OFFSET,
    cpu::dbt::{BlockSpec, CompileLimits, RegistryError, RegistryLimits, ResidentRegistry, UnitId},
    memory::GuestAddress,
};

impl EngineInstance {
    pub fn compile_resident(&mut self, count: u32) -> Result<UnitId, HostError> {
        self.memory()?;
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        Self::check_region_counts(count, 0)?;
        let mut specs = [BlockSpec {
            entry: GuestAddress(0),
            byte_length: 0,
        }; 8];
        let transfer = &self.arena()[TRANSFER_OFFSET..];
        for (index, spec) in specs[..count as usize].iter_mut().enumerate() {
            let offset = index * 8;
            *spec = BlockSpec {
                entry: GuestAddress(u32::from_le_bytes(
                    transfer[offset..offset + 4].try_into().unwrap(),
                )),
                byte_length: u32::from_le_bytes(
                    transfer[offset + 4..offset + 8].try_into().unwrap(),
                ),
            };
        }
        let memory = self.memory.as_ref().ok_or(HostError::Closed)?;
        let specs = &specs[..count as usize];
        if let Some(registry) = self.resident.as_mut() {
            registry
                .compile_bound(memory, specs, CompileLimits::default(), self.key)
                .map_err(HostError::Resident)
        } else {
            let mut registry = ResidentRegistry::new(memory, RegistryLimits::default())
                .map_err(HostError::Resident)?;
            let id = registry
                .compile_bound(memory, specs, CompileLimits::default(), self.key)
                .map_err(HostError::Resident)?;
            self.resident = Some(registry);
            Ok(id)
        }
    }

    pub fn lookup_resident(&self, pc: u32) -> Result<UnitId, HostError> {
        let memory = self.memory()?;
        let pc = GuestAddress(pc);
        self.resident
            .as_ref()
            .ok_or(HostError::Resident(RegistryError::NotFound { pc }))?
            .lookup(memory, pc)
            .map_err(HostError::Resident)
    }

    pub fn resident_bytes(&self, id: u64) -> Result<&[u8], HostError> {
        let memory = self.memory()?;
        let registry = self
            .resident
            .as_ref()
            .ok_or(HostError::Resident(RegistryError::InvalidUnit))?;
        registry
            .get_raw(memory, id)
            .map_err(HostError::Resident)?
            .wasm_bytes(memory)
            .map_err(|_| HostError::Resident(RegistryError::CodeInvalidated))
    }

    pub fn guard_resident(&self, key: u64, id: u64) -> Result<(), HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        self.resident_bytes(id)?;
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        Ok(())
    }
}
