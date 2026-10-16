use crate::{
    process::{EngineInstance, HostError},
    windows::{activation::ActivationGraph32, text::ordinal_key},
};
use std::sync::Arc;

#[derive(Clone)]
pub(in crate::process) struct ModuleActivationSnapshot32 {
    graphs: [Option<Arc<ActivationGraph32>>; 2],
}
impl ModuleActivationSnapshot32 {
    pub(in crate::process) fn from_optional_graph(
        graph: Option<Arc<ActivationGraph32>>,
        fallback: Option<Arc<ActivationGraph32>>,
    ) -> Self {
        Self {
            graphs: [graph, fallback],
        }
    }
    pub(in crate::process) fn from_graph(
        graph: Arc<ActivationGraph32>,
        fallback: Option<Arc<ActivationGraph32>>,
    ) -> Self {
        Self {
            graphs: [Some(graph), fallback],
        }
    }
    pub(in crate::process) fn dll_redirection(&self, name: &[u16]) -> Option<Vec<u16>> {
        let key = ordinal_key(name);
        for graph in self.graphs.iter().flatten() {
            for assembly in graph.assemblies() {
                for file in &assembly.manifest.files {
                    if ordinal_key(&file.name.encode_utf16().collect::<Vec<_>>()) == key {
                        let mut path = assembly.directory.clone();
                        if !path.ends_with(&[92]) {
                            path.push(92);
                        }
                        path.extend(
                            file.name
                                .encode_utf16()
                                .map(|unit| if unit == 47 { 92 } else { unit }),
                        );
                        return Some(path);
                    }
                }
            }
        }
        None
    }
}
impl EngineInstance {
    pub(in crate::process) fn capture_module_activation(
        &self,
    ) -> Result<ModuleActivationSnapshot32, HostError> {
        let mut graphs = [None, None];
        if let Some(runtime) = &self.activation {
            for (index, handle) in runtime
                .native
                .effective_lookup_handles(self.current_windows_thread_id()?)
                .into_iter()
                .enumerate()
            {
                if handle == 0 {
                    continue;
                }
                runtime
                    .storage(handle)
                    .map_err(HostError::Activation)?
                    .check(self.memory()?)
                    .map_err(HostError::Activation)?;
                graphs[index] = Some(
                    runtime
                        .native
                        .shared_graph(handle)
                        .map_err(HostError::Activation)?,
                );
            }
        }
        Ok(ModuleActivationSnapshot32 { graphs })
    }
}

impl EngineInstance {
    pub(in crate::process) fn capture_loaded_module_activations(
        &self,
    ) -> Result<Vec<(String, ModuleActivationSnapshot32)>, HostError> {
        let mut snapshots = Vec::new();
        let Some(runtime) = &self.activation else {
            return Ok(snapshots);
        };
        snapshots
            .try_reserve_exact(runtime.module_contexts.len())
            .map_err(|_| {
                HostError::Activation(
                    crate::windows::activation::ActivationError32::NotEnoughMemory,
                )
            })?;
        let default = runtime.native.process_default();
        let fallback = if default == 0 {
            None
        } else {
            Some(
                runtime
                    .native
                    .shared_graph(default)
                    .map_err(HostError::Activation)?,
            )
        };
        for &(id, handle) in &runtime.module_contexts {
            let Some(module) = self
                .loaded_windows_modules()
                .find(|module| module.id() == id)
            else {
                return Err(HostError::Infrastructure);
            };
            runtime
                .storage(handle)
                .map_err(HostError::Activation)?
                .check(self.memory()?)
                .map_err(HostError::Activation)?;
            snapshots.push((
                module.identity().to_owned(),
                ModuleActivationSnapshot32::from_graph(
                    runtime
                        .native
                        .shared_graph(handle)
                        .map_err(HostError::Activation)?,
                    if handle == default {
                        None
                    } else {
                        fallback.clone()
                    },
                ),
            ));
        }
        Ok(snapshots)
    }
}

impl EngineInstance {
    pub(in crate::process) fn capture_module_process_default_graph(
        &self,
    ) -> Result<Option<Arc<ActivationGraph32>>, HostError> {
        let Some(runtime) = &self.activation else {
            return Ok(None);
        };
        let handle = runtime.native.process_default();
        if handle == 0 {
            return Ok(None);
        }
        runtime
            .storage(handle)
            .map_err(HostError::Activation)?
            .check(self.memory()?)
            .map_err(HostError::Activation)?;
        runtime
            .native
            .shared_graph(handle)
            .map(Some)
            .map_err(HostError::Activation)
    }
}
