use super::ModuleSourceWait32;
use crate::{
    loader::ModuleError32,
    process::{EngineInstance, HostError},
};
impl EngineInstance {
    pub fn pending_module_source(&self) -> Option<ModuleSourceWait32> {
        let source = self.modules.source.as_ref()?;
        let request = source.bytes;
        Some(ModuleSourceWait32 {
            epoch: self.key,
            token: source.token,
            thread: source.thread,
            ready: source.ready(self),
            request,
        })
    }
    pub fn resume_module_source(
        &mut self,
        epoch: u64,
        token: u64,
    ) -> Result<super::super::ModuleOperation32, HostError> {
        self.memory()?;
        if epoch != self.key {
            return Err(HostError::InvalidArtifact);
        }
        let source = self
            .modules
            .source
            .as_ref()
            .filter(|source| source.token == token && token != 0)
            .ok_or(HostError::Module(ModuleError32::InvalidToken))?;
        if source.thread != self.current_windows_thread_id()? {
            return Err(HostError::Module(ModuleError32::Busy));
        }
        if !source.context.matches(self.arena()) {
            return Err(HostError::Call(crate::process::CallError::StateChanged));
        }
        if !source.ready(self) {
            return Err(HostError::Call(crate::process::CallError::Busy));
        }
        let source = self.modules.source.take().unwrap();
        let result = self.advance_module_source(source);
        if let Err(error) = result
            && self.modules.source.is_none()
        {
            if self.modules.loader_depth != 0 {
                self.release_loader_thread();
            }
            if self.complete_module_source_failure(error)? {
                return Ok(super::super::ModuleOperation32::InitializationFailed);
            }
        }
        if let Ok(operation) = result {
            self.complete_module_windows_operation(operation)?;
        }
        result
    }
    pub(in crate::process) fn module_source_provider_owns_token(&self, token: u32) -> bool {
        self.modules
            .source
            .as_ref()
            .is_some_and(|source| source.owns_token(token))
    }
    pub(in crate::process) fn module_source_ready(&self) -> bool {
        self.modules
            .source
            .as_ref()
            .is_some_and(|source| source.ready(self))
    }
}
