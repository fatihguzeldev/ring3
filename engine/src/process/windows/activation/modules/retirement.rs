use crate::{
    loader::ModuleId32,
    memory::PageRange,
    process::{EngineInstance, HostError},
    windows::activation::ActivationError32 as E,
};
pub(in crate::process) struct PreparedModuleActivationRetirement32 {
    revision: u64,
    ids: Vec<ModuleId32>,
    handles: Vec<u32>,
    disposed: Vec<u32>,
    ranges: Vec<PageRange>,
    initial: bool,
}
impl PreparedModuleActivationRetirement32 {
    pub(in crate::process) fn ranges(&self) -> &[PageRange] {
        &self.ranges
    }
}
impl EngineInstance {
    pub(in crate::process) fn prepare_module_activation_retirement(
        &self,
        ids: &[ModuleId32],
        initial: bool,
    ) -> Result<Option<PreparedModuleActivationRetirement32>, HostError> {
        let Some(runtime) = &self.activation else {
            return Ok(None);
        };
        let bindings = runtime
            .module_contexts
            .iter()
            .filter(|(id, _)| ids.contains(id));
        let mut handles = Vec::new();
        let mut owned_ids = Vec::new();
        for &(id, handle) in bindings {
            handles
                .try_reserve(1)
                .map_err(|_| HostError::Activation(E::NotEnoughMemory))?;
            owned_ids
                .try_reserve(1)
                .map_err(|_| HostError::Activation(E::NotEnoughMemory))?;
            handles.push(handle);
            owned_ids.push(id);
        }
        let disposed = runtime
            .native
            .check_release_holds(&handles, initial)
            .map_err(HostError::Activation)?;
        let ranges = runtime
            .disposal_ranges(self.memory()?, &disposed)
            .map_err(HostError::Activation)?;
        Ok(Some(PreparedModuleActivationRetirement32 {
            revision: runtime.native.revision(),
            ids: owned_ids,
            handles,
            disposed,
            ranges,
            initial,
        }))
    }
    pub(in crate::process) fn commit_module_activation_retirement(
        &mut self,
        prepared: Option<PreparedModuleActivationRetirement32>,
    ) {
        let Some(prepared) = prepared else {
            return;
        };
        let runtime = self.activation.as_mut().unwrap();
        assert_eq!(runtime.native.revision(), prepared.revision);
        runtime
            .native
            .release_holds(&prepared.handles, prepared.initial);
        runtime.retire_storage(&prepared.disposed);
        runtime
            .module_contexts
            .retain(|(id, _)| !prepared.ids.contains(id));
    }
}
