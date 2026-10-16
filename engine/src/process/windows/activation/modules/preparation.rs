use super::super::storage::{ContextStorage32, PreparedContextStorage32};
use super::ModuleActivationWork32;
use crate::{
    loader::{LoadPlan32, ModuleId32},
    process::{EngineInstance, HostError},
    windows::activation::{
        ActivationError32 as E, PreparedActivationGroup32, encode_activation_sections,
    },
};

pub(in crate::process) struct PreparedModuleActivation32 {
    pub(super) group: PreparedActivationGroup32,
    pub(super) bindings: Vec<(ModuleId32, u32)>,
    pub(super) storage: Vec<ContextStorage32>,
    pub(super) leases: Vec<crate::process::windows::files::ProviderFile32>,
}
impl EngineInstance {
    pub(in crate::process) fn stage_module_activation(
        &mut self,
        plan: &mut LoadPlan32,
        works: &[ModuleActivationWork32],
    ) -> Result<Option<PreparedModuleActivation32>, HostError> {
        if works.is_empty() {
            return Ok(None);
        }
        let all_works = works;
        let works = works
            .iter()
            .filter(|work| {
                !work.absent
                    && plan
                        .additions()
                        .iter()
                        .any(|module| module.identity() == work.identity)
            })
            .collect::<Vec<_>>();
        let runtime = self.activation.as_mut().ok_or(HostError::Infrastructure)?;
        runtime
            .module_contexts
            .try_reserve(works.len())
            .map_err(|_| HostError::Activation(E::NotEnoughMemory))?;
        let graphs = works
            .iter()
            .map(|work| {
                Ok((
                    work.work
                        .graph
                        .as_ref()
                        .ok_or(HostError::Infrastructure)?
                        .clone(),
                    work.process_default,
                ))
            })
            .collect::<Result<Vec<_>, HostError>>()?;
        let group = runtime
            .native
            .prepare_module_group(&graphs)
            .map_err(HostError::Activation)?;
        let mut bindings = Vec::new();
        let mut storage = Vec::new();
        bindings
            .try_reserve_exact(works.len())
            .map_err(|_| HostError::Activation(E::NotEnoughMemory))?;
        storage
            .try_reserve_exact(works.len())
            .map_err(|_| HostError::Activation(E::NotEnoughMemory))?;
        let mut used = runtime.used;
        let memory = self.memory.as_ref().ok_or(HostError::Closed)?;
        for (work, handle) in works.iter().zip(group.handles()) {
            let budget = (runtime.config.section_bytes as usize)
                .checked_sub(used)
                .ok_or(HostError::Activation(E::NotEnoughMemory))?;
            let sections =
                encode_activation_sections(work.work.graph.as_ref().unwrap(), handle, budget)
                    .map_err(HostError::Activation)?;
            let prepared = PreparedContextStorage32::prepare(
                memory,
                runtime.config.section_bounds,
                handle,
                sections,
                work.work.source_cost,
                budget,
            )
            .map_err(HostError::Activation)?;
            let prepared = prepared
                .stage(memory, runtime.config.section_bounds, plan)
                .map_err(HostError::Activation)?;
            used += prepared.cost;
            storage.push(prepared);
            let module = plan
                .additions()
                .iter()
                .find(|module| module.identity() == work.identity)
                .ok_or(HostError::Infrastructure)?;
            bindings.push((module.id(), handle));
        }
        runtime
            .native
            .can_commit_group(&group)
            .map_err(HostError::Activation)?;
        let mut leases = Vec::new();
        leases
            .try_reserve_exact(all_works.iter().map(|work| work.work.leases.len()).sum())
            .map_err(|_| HostError::Activation(E::NotEnoughMemory))?;
        for work in all_works {
            leases.extend_from_slice(&work.work.leases);
        }
        self.check_close_provider_files(&leases)?;
        Ok(Some(PreparedModuleActivation32 {
            group,
            bindings,
            storage,
            leases,
        }))
    }
}
