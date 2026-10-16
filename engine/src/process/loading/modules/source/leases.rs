use super::PendingModuleLoad32;
use crate::{
    loader::{LoadPlan32, ModuleError32},
    process::{EngineInstance, HostError, windows::files::ProviderFile32},
};

pub(super) struct ModuleSourceLease32 {
    pub(super) identity: String,
    pub(super) lease: ProviderFile32,
}

impl EngineInstance {
    pub(super) fn retain_module_source(
        &mut self,
        source: &mut PendingModuleLoad32,
        identity: &str,
    ) -> Result<(), HostError> {
        if self.modules.loaded.find(identity).is_some()
            || source.leases.iter().any(|lease| lease.identity == identity)
        {
            return Ok(());
        }
        let Some(version) = self
            .modules
            .vfs_sources
            .iter()
            .find(|cached| cached.identity == identity)
            .map(|cached| cached.version)
        else {
            return Ok(());
        };
        let mut name = String::new();
        name.try_reserve_exact(identity.len())
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        name.push_str(identity);
        source
            .leases
            .try_reserve(1)
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        let lease = self.open_module_source_file(
            &name.encode_utf16().collect::<Vec<_>>(),
            source.request.file_section(),
        )?;
        source.leases.push(ModuleSourceLease32 {
            identity: name,
            lease,
        });
        if self.provider_file_version(lease)? != version {
            return Err(HostError::Module(ModuleError32::StalePlan));
        }
        Ok(())
    }

    pub(super) fn retain_module_plan_sources(
        &mut self,
        source: &mut PendingModuleLoad32,
        plan: &LoadPlan32,
    ) -> Result<(), HostError> {
        for module in plan.additions() {
            self.retain_module_source(source, module.identity())?;
        }
        Ok(())
    }

    pub(super) fn module_source_leases(
        &self,
        source: &PendingModuleLoad32,
    ) -> Result<Vec<ProviderFile32>, HostError> {
        let count = usize::from(source.input.is_some())
            + source.leases.len()
            + source
                .works
                .as_ref()
                .into_iter()
                .flatten()
                .map(|work| work.leases().len())
                .sum::<usize>();
        let mut leases = Vec::new();
        leases
            .try_reserve_exact(count)
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        if let Some(input) = &source.input {
            leases.push(input.lease);
        }
        leases.extend(source.leases.iter().map(|source| source.lease));
        for work in source.works.as_ref().into_iter().flatten() {
            leases.extend_from_slice(work.leases());
        }
        Ok(leases)
    }
}
