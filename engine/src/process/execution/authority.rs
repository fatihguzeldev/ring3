use crate::cpu::dbt::{CompiledRegion, RegistryError};
use crate::process::{CallError, EngineInstance, HostError, calls::callback::SuspendedCallback};

impl EngineInstance {
    pub(in crate::process) fn guard_detection_material_change(&self) -> Result<(), HostError> {
        #[cfg(any(target_arch = "wasm32", test))]
        if self.has_retained_detection() {
            return Err(HostError::Call(CallError::Busy));
        }
        Ok(())
    }

    pub fn guard_dispatch_entry(&self, key: u64) -> Result<(), HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        self.guard_thread_execution_ready()?;
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
        self.guard_thread_execution_ready()?;
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

    fn guard_thread_execution_ready(&self) -> Result<(), HostError> {
        if self.pending_windows_thread_schedule().is_some()
            || self.exceptions.thread_unhandled(self.threads.current_id())
            || self
                .threads
                .info(self.threads.current_id())
                .is_ok_and(|thread| thread.suspend_count != 0)
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
        self.guard_thread_execution_ready()?;
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

    pub(in crate::process) fn check_decode_key(
        &self,
        artifact: &CompiledRegion,
    ) -> Result<(), HostError> {
        if !artifact.matches_decode_key(self.code_decode_key()?) {
            return Err(HostError::InvalidArtifact);
        }
        Ok(())
    }

    pub(in crate::process) fn guard_material(
        &self,
        key: u64,
        generation: u32,
    ) -> Result<(), HostError> {
        self.guard(key, generation)?;
        self.check_decode_key(self.artifact.as_ref().unwrap())
    }

    pub(in crate::process) fn guard_material_resident(
        &self,
        key: u64,
        id: u64,
    ) -> Result<(), HostError> {
        self.guard_resident(key, id)?;
        self.check_decode_key(self.guard_resident_unit(key, id)?)
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

#[cfg(test)]
mod decode_key_tests {
    use super::*;
    use crate::{abi::arena::TRANSFER_OFFSET, cpu::x86::SegmentRegister};

    #[test]
    fn wrong_key_store_is_refused_without_conflating_physical_artifact_currentness() {
        let key = 0x4353;
        let mut engine = EngineInstance::new(3, key).unwrap();
        for page in [0x1000, 0x5000, 0x7000] {
            engine.map(page, 1, 7).unwrap();
        }
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
            .copy_from_slice(&[0xeb, 0xfe]);
        engine.upload(0x1000, 2).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
            .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
        engine.compile(1).unwrap();
        engine
            .set_segment_base(SegmentRegister::Cs, 0x4000)
            .unwrap();
        let before = engine.arena().to_vec();
        assert_eq!(engine.guard(key, 1), Ok(()));
        assert_eq!(
            engine.guard_material(key, 1),
            Err(HostError::InvalidArtifact)
        );
        assert_eq!(
            engine.store32(0x7000, 0x1234_5678),
            Err(HostError::InvalidArtifact)
        );
        assert_eq!(engine.arena(), before);
        engine.read32(0x7000).unwrap();
        assert_eq!(
            u32::from_le_bytes(engine.arena()[120..124].try_into().unwrap()),
            0
        );
    }
}
