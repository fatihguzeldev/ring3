use super::initialization::ModuleOperation;
use crate::process::{EngineInstance, HostError};

pub(in crate::process) struct ModuleThreadState32 {
    operations: Vec<ModuleOperation>,
    waiting_request: Option<ModuleRequest32>,
    source: Option<super::source::PendingModuleLoad32>,
}
impl ModuleThreadState32 {
    pub(in crate::process) fn execution_pins(&self) -> impl Iterator<Item = u64> + '_ {
        super::callback::operation_execution_pins(&self.operations)
    }
    pub(in crate::process) fn source_ready(&self, engine: &EngineInstance) -> bool {
        self.source
            .as_ref()
            .is_some_and(|source| source.ready(engine))
    }
    pub(in crate::process) fn empty() -> Self {
        Self {
            operations: Vec::new(),
            waiting_request: None,
            source: None,
        }
    }
}
impl EngineInstance {
    pub(in crate::process) fn take_module_thread_state(&mut self) -> ModuleThreadState32 {
        ModuleThreadState32 {
            operations: std::mem::take(&mut self.modules.operations),
            waiting_request: self.modules.waiting_request.take(),
            source: self.modules.source.take(),
        }
    }
    pub(in crate::process) fn restore_module_thread_state(&mut self, state: ModuleThreadState32) {
        assert!(
            self.modules.operations.is_empty(),
            "active loader continuation must be parked before thread restore"
        );
        self.modules.operations = state.operations;
        assert!(self.modules.waiting_request.is_none());
        self.modules.waiting_request = state.waiting_request;
        assert!(self.modules.source.is_none());
        self.modules.source = state.source;
    }
    pub(in crate::process) fn loader_initialization_owner(&self) -> Option<u32> {
        self.modules.loader_owner
    }
    pub(in crate::process) fn acquire_loader_thread(&mut self) -> Result<(), HostError> {
        let current = self.current_windows_thread_id()?;
        if self
            .modules
            .loader_owner
            .is_some_and(|owner| owner != current)
        {
            return Err(HostError::Call(crate::process::CallError::Busy));
        }
        let next = self
            .modules
            .loader_depth
            .checked_add(1)
            .ok_or(HostError::Infrastructure)?;
        self.modules.loader_owner = Some(current);
        self.modules.loader_depth = next;
        Ok(())
    }
    pub(in crate::process) fn release_loader_thread(&mut self) {
        assert!(self.modules.loader_depth != 0);
        self.modules.loader_depth -= 1;
        if self.modules.loader_depth == 0 {
            self.modules.loader_owner = None;
        }
    }
    pub(in crate::process) fn release_idle_loader_thread(&mut self) {
        if self.modules.operations.is_empty() {
            self.release_loader_thread();
        }
    }
}

impl EngineInstance {
    /// Build callbacks from live image memory while the caller owns the loader lock.
    pub(in crate::process) fn windows_thread_initializers(
        &self,
        reason: u32,
    ) -> Result<Vec<(crate::memory::GuestAddress, [u32; 3])>, HostError> {
        use crate::{
            loader::ModuleError32,
            memory::{Access, GuestAddress},
        };
        if !matches!(reason, 2 | 3) {
            return Err(HostError::Module(ModuleError32::InvalidRequest));
        }
        let memory = self.memory()?;
        let limit = self.modules.catalog.limits().parser.metadata_entries;
        let mut calls = Vec::new();
        for offset in 0..self.modules.attached.len() {
            let index = if reason == 3 {
                self.modules.attached.len() - 1 - offset
            } else {
                offset
            };
            let id = self.modules.attached[index];
            let module = self
                .modules
                .loaded
                .get(id)
                .ok_or(HostError::Module(ModuleError32::InvalidHandle))?;
            if self.modules.thread_notifications_disabled.contains(&id) {
                continue;
            }
            let args = [module.base(), reason, 0];
            if let Some(array) = module.tls().and_then(|tls| tls.callbacks_address) {
                let mut terminated = false;
                for index in 0..limit {
                    let address = u32::try_from(index)
                        .ok()
                        .and_then(|v| v.checked_mul(4))
                        .and_then(|v| array.0.checked_add(v))
                        .ok_or(HostError::Module(ModuleError32::Capacity))?;
                    let mut bytes = [0; 4];
                    memory
                        .read(GuestAddress(address), &mut bytes)
                        .map_err(HostError::Memory)?;
                    let entry = u32::from_le_bytes(bytes);
                    if entry == 0 {
                        terminated = true;
                        break;
                    }
                    memory
                        .check_span(GuestAddress(entry), 1, Access::Execute)
                        .map_err(HostError::Memory)?;
                    if calls.len() >= limit {
                        return Err(HostError::Module(ModuleError32::Capacity));
                    }
                    calls
                        .try_reserve(1)
                        .map_err(|_| HostError::Infrastructure)?;
                    calls.push((GuestAddress(entry), args));
                }
                if !terminated {
                    return Err(HostError::Module(ModuleError32::Capacity));
                }
            }
            if module.is_dll()
                && let Some(entry) = module.entry()
            {
                memory
                    .check_span(entry, 1, Access::Execute)
                    .map_err(HostError::Memory)?;
                if calls.len() >= limit {
                    return Err(HostError::Module(ModuleError32::Capacity));
                }
                calls
                    .try_reserve(1)
                    .map_err(|_| HostError::Infrastructure)?;
                calls.push((entry, args));
            }
        }
        Ok(calls)
    }
    pub(in crate::process) fn disable_windows_thread_notifications(
        &mut self,
        base: u32,
    ) -> Result<(), HostError> {
        use crate::loader::ModuleError32;
        let module = self
            .modules
            .loaded
            .iter()
            .find(|module| module.base() == base && module.is_dll())
            .ok_or(HostError::Module(ModuleError32::InvalidHandle))?;
        if module.tls().is_some() {
            return Err(HostError::Module(ModuleError32::InvalidRequest));
        }
        let id = module.id();
        if !self.modules.thread_notifications_disabled.contains(&id) {
            self.modules
                .thread_notifications_disabled
                .try_reserve(1)
                .map_err(|_| HostError::Infrastructure)?;
            self.modules.thread_notifications_disabled.push(id);
        }
        Ok(())
    }
}

pub(in crate::process) enum ModuleRequest32 {
    Load {
        name: String,
        flags: u32,
    },
    Export {
        handle: u32,
        symbol: crate::loader::ImportSymbol32,
    },
    Free {
        handle: u32,
    },
    Shutdown,
}
impl EngineInstance {
    pub(in crate::process) fn park_module_request(
        &mut self,
        request: ModuleRequest32,
    ) -> Result<(), HostError> {
        if self.modules.waiting_request.is_some() {
            return Err(HostError::Call(crate::process::CallError::Busy));
        }
        self.modules.waiting_request = Some(request);
        Ok(())
    }
    pub(in crate::process) fn take_module_request(&mut self) -> Option<ModuleRequest32> {
        self.modules.waiting_request.take()
    }
    pub(in crate::process) fn execute_module_request(
        &mut self,
        request: &ModuleRequest32,
    ) -> Result<super::ModuleOperation32, HostError> {
        match request {
            ModuleRequest32::Load { name, flags } => {
                if *flags == 0 {
                    self.load_library_module(name, None)
                } else {
                    self.load_resource_only_module(name, *flags)
                }
            }
            ModuleRequest32::Export { handle, symbol } => {
                self.resolve_module_export(*handle, symbol)
            }
            ModuleRequest32::Free { handle } => {
                if *handle & 3 != 0 {
                    self.free_resource_only_module(*handle)
                } else {
                    self.free_library_module(*handle)
                }
            }
            ModuleRequest32::Shutdown => self.detach_modules_for_exit(),
        }
    }
}

impl EngineInstance {
    pub(in crate::process) fn finalize_last_windows_thread(
        &mut self,
        code: u32,
    ) -> Result<bool, HostError> {
        if self.exit_code.is_some() {
            return Ok(true);
        }
        if !self.threads.is_last_live() {
            return Ok(true);
        }
        if self.has_active_module_operation() {
            return Ok(false);
        }
        if !self.advance_windows_fls_process_exit()? {
            return Ok(false);
        }
        if !self.modules.attached.is_empty()
            && matches!(
                self.detach_modules_for_exit()?,
                super::ModuleOperation32::Pending(_)
            )
        {
            return Ok(false);
        }
        self.publish_process_exit(code)?;
        Ok(self.exit_code.is_some())
    }
}

impl EngineInstance {
    pub(in crate::process) fn finalize_forced_last_windows_thread(
        &mut self,
        code: u32,
    ) -> Result<bool, HostError> {
        self.publish_process_exit(code)?;
        Ok(self.exit_code.is_some())
    }
}
