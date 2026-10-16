use crate::process::{
    EngineInstance, HostError,
    calls::{call::PendingOwner, callback::SuspendedRecord},
};

fn owner(output: &mut Vec<u64>, value: PendingOwner) {
    match value {
        PendingOwner::Resident(id) | PendingOwner::ResidentCallback { unit_id: id, .. } => {
            output.push(id)
        }
        PendingOwner::Replacement(_) => {}
    }
}
impl EngineInstance {
    pub(in crate::process::execution) fn cache_pins(&self) -> Result<Vec<u64>, HostError> {
        let depth = self
            .callback
            .as_ref()
            .map_or(0, |callback| callback.depth());
        let saved = self.threads.execution_pins().count()
            + self.exceptions.execution_pins().count()
            + self.modules.execution_pins().count();
        let capacity = (depth * 4
            + self.windows_replies.len()
            + self.ole32_pending_owners().count()
            + self.automation_pending_owners().count()
            + 8)
        .checked_add(saved)
        .ok_or(HostError::Resident(
            crate::cpu::dbt::RegistryError::Allocation,
        ))?;
        let mut pins = Vec::new();
        pins.try_reserve_exact(capacity)
            .map_err(|_| HostError::Resident(crate::cpu::dbt::RegistryError::Allocation))?;
        if let Some(call) = &self.pending_call {
            owner(&mut pins, call.owner);
        }
        for value in self.ole32_pending_owners() {
            owner(&mut pins, value);
        }
        for value in self.automation_pending_owners() {
            owner(&mut pins, value);
        }
        if let Some(value) = self.version.pending_owner() {
            owner(&mut pins, value);
        }
        if let Some(value) = self.winmm.pending_owner() {
            owner(&mut pins, value);
        }
        if let Some(value) = self
            .dinput
            .as_ref()
            .and_then(|runtime| runtime.pending_file_owner())
        {
            owner(&mut pins, value);
        }
        if let Some(event) = &self.cpu_event {
            owner(&mut pins, event.owner);
        }
        if let Some(reply) = &self.module_windows_reply {
            owner(&mut pins, reply.owner());
        }
        for reply in &self.windows_replies {
            owner(&mut pins, reply.owner());
        }
        let mut callback = self.callback.as_ref();
        while let Some(current) = callback {
            owner(&mut pins, current.outer.owner);
            if let SuspendedRecord::Resident {
                record,
                active_unit_id,
                ..
            } = current.record
            {
                pins.extend([
                    record.outer_unit_id,
                    record.callback_unit_id,
                    active_unit_id,
                ]);
            }
            callback = current.parent.as_ref().map(|parent| &parent[0]);
        }
        pins.extend(self.threads.execution_pins());
        pins.extend(self.exceptions.execution_pins());
        pins.extend(self.modules.execution_pins());
        if let Some(registry) = &self.registry
            && let Some(value) = registry.pending_owner()
        {
            owner(&mut pins, value);
        }
        if let Some(value) = self
            .activation
            .as_ref()
            .and_then(|runtime| runtime.pending_owner())
        {
            owner(&mut pins, value);
        }
        pins.sort_unstable();
        pins.dedup();
        Ok(pins)
    }
    pub(super) fn guard_cache_control(&self, key: u64) -> Result<(), HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        self.guard_detection_material_change()?;
        if self.call_cancelled() {
            return Err(HostError::Call(crate::process::CallError::Cancelled));
        }
        Ok(())
    }
}
