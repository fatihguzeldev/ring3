use super::{ExecutionCache32, ExecutionConfig32};
use crate::{
    cpu::dbt::{ResidentRegistry, emit_dispatcher_with_capacity},
    process::{CallError, EngineInstance, HostError},
};

impl EngineInstance {
    pub fn configure_execution(&mut self, config: ExecutionConfig32) -> Result<(), HostError> {
        self.memory()?;
        if self.execution_cache.is_some()
            || self.artifact.is_some()
            || self.resident.is_some()
            || self.pending_call.is_some()
            || self.callback.is_some()
        {
            return Err(HostError::Call(CallError::Busy));
        }
        self.guard_detection_material_change()?;
        let cache = ExecutionCache32::new(config).map_err(HostError::Resident)?;
        let registry =
            ResidentRegistry::new(self.memory.as_ref().unwrap(), cache.registry_limits())
                .map_err(HostError::Resident)?;
        let mut installations = Vec::new();
        installations
            .try_reserve_exact(config.units as usize)
            .map_err(|_| HostError::Resident(crate::cpu::dbt::RegistryError::Allocation))?;
        installations.resize(config.units as usize, None);
        let mut installations_by_id = Vec::new();
        installations_by_id
            .try_reserve_exact(config.units as usize)
            .map_err(|_| HostError::Resident(crate::cpu::dbt::RegistryError::Allocation))?;
        let dispatcher = emit_dispatcher_with_capacity(self.key, config.units);
        self.execution_cache = Some(cache);
        self.resident = Some(registry);
        self.resident_installations = installations;
        self.resident_installations_by_id = installations_by_id;
        self.dispatcher = Some(dispatcher);
        Ok(())
    }
    pub fn execution_config(&self) -> Result<Option<ExecutionConfig32>, HostError> {
        self.memory()?;
        Ok(self.execution_cache.as_ref().map(|cache| cache.config))
    }
}
