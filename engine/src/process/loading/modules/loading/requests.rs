use super::super::{CapturedModuleRequest32, ModuleOperation32, ModuleRuntimeConfig32};
use crate::{
    abi::x86::decode_state,
    cpu::x86::State32,
    loader::ModuleError32,
    process::{EngineInstance, HostError},
};

impl EngineInstance {
    pub fn start_module_process(
        &mut self,
        name: &str,
        actual_base: Option<u32>,
        config: ModuleRuntimeConfig32,
    ) -> Result<ModuleOperation32, HostError> {
        #[cfg(feature = "observability")]
        let _capture = self.observer.enter();
        #[cfg(feature = "observability")]
        let _scope = ring3_observability::current().scope(
            ring3_observability::Level::Basic,
            ring3_observability::categories::ENGINE_LOADER_MODULES,
            "start_process",
            &[ring3_observability::Field::string("module", name)],
        );
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
        self.load_module_request(name, actual_base, Some(config), false, false)
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
        self.load_module_request(name, actual_base, None, false, false)
    }

    pub(in crate::process) fn load_library_module_flags(
        &mut self,
        name: &str,
        flags: u32,
    ) -> Result<ModuleOperation32, HostError> {
        if flags & !9 != 0 {
            return self.load_resource_only_module(name, flags);
        }
        if self.modules.thread.is_none() {
            return Err(HostError::Module(ModuleError32::InvalidRequest));
        }
        self.load_module_request(name, None, None, flags & 1 != 0, flags & 8 != 0)
    }

    fn load_module_request(
        &mut self,
        name: &str,
        actual_base: Option<u32>,
        config: Option<ModuleRuntimeConfig32>,
        no_resolve: bool,
        altered: bool,
    ) -> Result<ModuleOperation32, HostError> {
        self.memory()?;
        self.guard_detection_material_change()?;
        let before = self.modules.loader_depth;
        self.acquire_loader_thread()?;
        let result = self.load_module_request_inner(name, actual_base, config, no_resolve, altered);
        if !matches!(
            result,
            Ok(ModuleOperation32::Pending(_) | ModuleOperation32::NeedBytes { .. })
        ) && self.modules.loader_depth > before
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
        no_resolve: bool,
        altered: bool,
    ) -> Result<ModuleOperation32, HostError> {
        self.memory()?;
        self.guard_detection_material_change()?;
        if self.call_cancelled() {
            return Err(HostError::Call(crate::process::CallError::Cancelled));
        }
        self.check_module_execution_admission()?;
        let paths = self.capture_module_paths((config.is_some() || altered).then_some(name))?;
        let name = self.resolve_module_source_captured(name, &paths)?;
        self.begin_module_source_request(CapturedModuleRequest32::Load {
            name,
            actual_base,
            config,
            no_resolve,
            paths,
        })
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
        if !matches!(
            result,
            Ok(ModuleOperation32::Pending(_) | ModuleOperation32::NeedBytes { .. })
        ) && self.modules.loader_depth > before
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
        let paths = self.capture_module_paths(None)?;
        self.begin_module_source_request(CapturedModuleRequest32::Export {
            handle,
            symbol: symbol.clone(),
            paths,
        })
    }
}
