use super::PreparedModuleActivation32;
use crate::process::EngineInstance;

impl EngineInstance {
    pub(in crate::process) fn commit_module_activation(
        &mut self,
        prepared: Option<PreparedModuleActivation32>,
    ) {
        let Some(mut prepared) = prepared else {
            return;
        };
        self.commit_close_provider_files(&prepared.leases);
        let memory = self.memory.as_ref().unwrap();
        for storage in &mut prepared.storage {
            storage.adopt_mapping(memory);
        }
        let runtime = self
            .activation
            .as_mut()
            .expect("module activation was staged");
        runtime.native.commit_module_group(prepared.group);
        for storage in &prepared.storage {
            runtime.used += storage.cost;
        }
        runtime.storage.extend(prepared.storage);
        runtime.module_contexts.extend(prepared.bindings);
    }
}
