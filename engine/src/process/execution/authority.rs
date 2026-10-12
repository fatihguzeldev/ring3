use crate::cpu::dbt::{CompiledRegion, RegistryError};
use crate::process::{CallError, EngineInstance, HostError, calls::callback::SuspendedCallback};

impl EngineInstance {
    pub fn guard_dispatch_entry(&self, key: u64) -> Result<(), HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        if self.pending_call.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        if let Some(callback) = self.callback.as_ref() {
            let Some(record) = callback.authorized_resident_record() else {
                return Err(HostError::Call(CallError::Busy));
            };
            self.guard_resident_unit(key, record.callback_unit_id)?;
            self.guard_resident_unit(key, record.outer_unit_id)?;
            self.guard_resident_unit(key, callback.authorized_resident_active_id().unwrap())?;
        }
        Ok(())
    }

    pub fn guard(&self, key: u64, generation: u32) -> Result<(), HostError> {
        self.guard_artifact(key, generation)?;
        if self.pending_call.is_some()
            || self
                .callback
                .as_ref()
                .is_some_and(SuspendedCallback::is_resident)
        {
            return Err(HostError::Call(CallError::Busy));
        }
        Ok(())
    }

    pub(in crate::process) fn guard_artifact(
        &self,
        key: u64,
        generation: u32,
    ) -> Result<(), HostError> {
        self.memory()?;
        if key != self.key || generation == 0 || generation != self.generation() {
            return Err(HostError::InvalidArtifact);
        }
        self.artifact_bytes().map(|_| ())
    }

    pub fn guard_resident(&self, key: u64, id: u64) -> Result<(), HostError> {
        self.guard_resident_unit(key, id)?;
        if self.pending_call.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        if let Some(callback) = self.callback.as_ref() {
            let Some(record) = callback.authorized_resident_record() else {
                return Err(HostError::Call(CallError::Busy));
            };
            if Some(id) != callback.authorized_resident_active_id() {
                return Err(HostError::Call(CallError::Busy));
            }
            self.guard_resident_unit(key, record.callback_unit_id)?;
            self.guard_resident_unit(key, record.outer_unit_id)?;
        }
        Ok(())
    }

    pub(in crate::process) fn guard_resident_unit(
        &self,
        key: u64,
        id: u64,
    ) -> Result<&CompiledRegion, HostError> {
        let memory = self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        self.resident
            .as_ref()
            .ok_or(HostError::Resident(RegistryError::InvalidUnit))?
            .get_raw(memory, id)
            .map_err(HostError::Resident)
    }
}
