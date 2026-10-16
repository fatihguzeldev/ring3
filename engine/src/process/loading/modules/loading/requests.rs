use super::super::{
    ModuleOperation32, ModuleRuntimeConfig32, initialization::LoadCompletion, tls::InitialThreadTls,
};
use crate::{
    abi::x86::decode_state,
    cpu::x86::State32,
    loader::ModuleError32,
    memory::{GuestAddress, PageRange},
    process::{EngineInstance, HostError},
};

impl EngineInstance {
    pub fn start_module_process(
        &mut self,
        name: &str,
        actual_base: Option<u32>,
        config: ModuleRuntimeConfig32,
    ) -> Result<ModuleOperation32, HostError> {
        self.module_process_capacity()?;
        if self.launch_input_pending() {
            return Err(HostError::Module(ModuleError32::Busy));
        }
        if decode_state(&self.arena()[..56]).map_err(|_| HostError::Infrastructure)?
            != State32::default()
            || !crate::process::cpu::context::ExtendedCpuState::initial()?.matches(self.arena())
        {
            return Err(HostError::Module(ModuleError32::InvalidRequest));
        }
        if self.modules.loaded.iter().len() != 0 {
            return Err(HostError::Module(ModuleError32::Busy));
        }
        self.load_module_request(name, actual_base, Some(config))
    }

    pub fn load_library_module(
        &mut self,
        name: &str,
        actual_base: Option<u32>,
    ) -> Result<ModuleOperation32, HostError> {
        self.memory()?;
        if self.modules.thread.is_none() {
            return Err(HostError::Module(ModuleError32::InvalidRequest));
        }
        self.load_module_request(name, actual_base, None)
    }

    fn load_module_request(
        &mut self,
        name: &str,
        actual_base: Option<u32>,
        config: Option<ModuleRuntimeConfig32>,
    ) -> Result<ModuleOperation32, HostError> {
        self.memory()?;
        self.guard_detection_material_change()?;
        let before = self.modules.loader_depth;
        self.acquire_loader_thread()?;
        let result = self.load_module_request_inner(name, actual_base, config);
        if !matches!(result, Ok(ModuleOperation32::Pending(_)))
            && self.modules.loader_depth > before
        {
            self.release_loader_thread();
        }
        result
    }

    fn load_module_request_inner(
        &mut self,
        name: &str,
        actual_base: Option<u32>,
        config: Option<ModuleRuntimeConfig32>,
    ) -> Result<ModuleOperation32, HostError> {
        self.memory()?;
        self.guard_detection_material_change()?;
        if self.call_cancelled() {
            return Err(HostError::Call(crate::process::CallError::Cancelled));
        }
        self.check_module_execution_admission()?;
        let mut reservations = self.guest_address_reservations()?;
        if let Some(config) = config {
            let slots = self
                .modules
                .catalog
                .limits()
                .modules
                .min(self.memory()?.capacity_pages() as usize) as u32;
            let thread = InitialThreadTls::new(config, slots).map_err(HostError::Module)?;
            for (start, end) in thread.reservations() {
                let range = PageRange::new(
                    GuestAddress(start as u32),
                    ((end - start) / u64::from(crate::memory::PAGE_SIZE)) as u32,
                )
                .map_err(HostError::Memory)?;
                if self
                    .provider_guest_arenas()
                    .any(|old| crate::process::memory::reservations::overlaps(old, range))
                {
                    return Err(HostError::Module(ModuleError32::AddressConflict));
                }
                reservations.push(range);
            }
        }
        let plan = self
            .modules
            .catalog
            .plan_load_with_runtime_dependencies(
                &self.modules.loaded,
                self.memory()?,
                name,
                actual_base,
                &reservations,
                if config.is_some() && self.modules.windows_runtime {
                    &["ntdll.dll"]
                } else {
                    &[]
                },
            )
            .map_err(HostError::Module)?;
        self.install_module_plan(plan, config, LoadCompletion::Library)
    }

    pub fn resolve_module_export(
        &mut self,
        handle: u32,
        symbol: &crate::loader::ImportSymbol32,
    ) -> Result<ModuleOperation32, HostError> {
        self.memory()?;
        self.guard_detection_material_change()?;
        let before = self.modules.loader_depth;
        self.acquire_loader_thread()?;
        let result = self.resolve_module_export_inner(handle, symbol);
        if !matches!(result, Ok(ModuleOperation32::Pending(_)))
            && self.modules.loader_depth > before
        {
            self.release_loader_thread();
        }
        result
    }

    fn resolve_module_export_inner(
        &mut self,
        handle: u32,
        symbol: &crate::loader::ImportSymbol32,
    ) -> Result<ModuleOperation32, HostError> {
        self.memory()?;
        self.guard_detection_material_change()?;
        if self.call_cancelled() {
            return Err(HostError::Call(crate::process::CallError::Cancelled));
        }
        self.check_module_execution_admission()?;
        let id = self
            .modules
            .loaded
            .iter()
            .find(|module| module.base() == handle)
            .ok_or(HostError::Module(ModuleError32::InvalidHandle))?
            .id();
        if let Some(resolved) = self
            .modules
            .loaded
            .direct_export_live(self.memory()?, id, symbol)
            .map_err(HostError::Module)?
        {
            return Ok(ModuleOperation32::ExportResolved {
                address: resolved.address().0,
            });
        }
        let reservations = self.guest_address_reservations()?;
        let (plan, resolved) = self
            .modules
            .catalog
            .plan_export_live_with_reservations(
                &self.modules.loaded,
                self.memory()?,
                id,
                symbol,
                &reservations,
            )
            .map_err(HostError::Module)?;
        self.install_module_plan(plan, None, LoadCompletion::Export(resolved.address().0))
    }
}
