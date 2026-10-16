use super::*;
use crate::process::{CallError, EngineInstance, HostError};
impl EngineInstance {
    pub fn configure_registry(&mut self, config: RegistryConfig32) -> Result<(), HostError> {
        self.memory()?;
        if self.registry.is_some() {
            return Err(HostError::Registry(RegistryError32::AlreadyConfigured));
        }
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        let epoch = crate::process::media::next_session()?;
        self.registry = Some(RegistryRuntime32::new(epoch, config).map_err(HostError::Registry)?);
        Ok(())
    }
    pub fn register_advapi32_module(&mut self, base: u32) -> Result<(), HostError> {
        self.register_system_module(crate::windows::registry::advapi32_module(base))
    }
    pub fn registry_seed(&mut self, seed: RegistrySeed32<'_>) -> Result<(), HostError> {
        self.memory()?;
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        let runtime = self
            .registry
            .as_mut()
            .ok_or(HostError::Registry(RegistryError32::NotConfigured))?;
        let prepared = runtime
            .native
            .prepare_seed(seed)
            .map_err(HostError::Registry)?;
        let plan = runtime
            .prepare_notifications(
                &prepared,
                self.synchronization.as_ref().map(|sync| sync.context()),
            )
            .map_err(HostError::Registry)?;
        runtime.native.commit(prepared);
        runtime.commit_notifications(
            plan,
            self.synchronization.as_mut().map(|sync| sync.context_mut()),
        );
        Ok(())
    }
    pub fn registry_epoch(&self) -> Result<u64, HostError> {
        Ok(self.registry_storage()?.native.epoch())
    }
    pub fn registry_change(&self, index: u32) -> Result<Option<RegistryChange32<'_>>, HostError> {
        self.registry_storage()?
            .native
            .change(index)
            .map_err(HostError::Registry)
    }
    pub fn registry_read_change(
        &self,
        id: RegistryChangeId32,
        offset: u32,
        output: &mut [u8],
    ) -> Result<usize, HostError> {
        self.registry_storage()?
            .native
            .read_change(id, offset, output)
            .map_err(HostError::Registry)
    }
    pub fn registry_read_change_text(
        &self,
        id: RegistryChangeId32,
        part: u32,
        offset: u32,
        output: &mut [u16],
    ) -> Result<usize, HostError> {
        self.registry_storage()?
            .native
            .read_change_text(id, part, offset, output)
            .map_err(HostError::Registry)
    }
    pub fn registry_acknowledge_change(&mut self, id: RegistryChangeId32) -> Result<(), HostError> {
        if !self.is_open() {
            return Err(HostError::Closed);
        }
        self.registry
            .as_mut()
            .ok_or(HostError::Registry(RegistryError32::NotConfigured))?
            .native
            .acknowledge_change(id)
            .map_err(HostError::Registry)
    }
    fn registry_storage(&self) -> Result<&RegistryRuntime32, HostError> {
        if !self.is_open() {
            return Err(HostError::Closed);
        }
        self.registry
            .as_ref()
            .ok_or(HostError::Registry(RegistryError32::NotConfigured))
    }
}
