use crate::{
    abi::arena::TRANSFER_SIZE,
    loader::{ModuleError32, normalize_module_identity},
    process::{EngineInstance, HostError},
};

pub(super) struct ModuleInput32 {
    name: String,
    total: u32,
    bytes: Vec<u8>,
}

impl EngineInstance {
    pub fn begin_module_input(&mut self, total: u32, name: &str) -> Result<(), HostError> {
        self.module_input_guard()?;
        if self.modules.input.is_some() {
            return Err(HostError::Module(ModuleError32::Busy));
        }
        if total == 0 || total as usize > self.modules.catalog.remaining_source_bytes() {
            return Err(HostError::Module(ModuleError32::Capacity));
        }
        let normalized = normalize_module_identity(name).map_err(HostError::Module)?;
        if self.modules.catalog.contains(&normalized) {
            return Err(HostError::Module(ModuleError32::DuplicateModule));
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(total as usize)
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        self.modules.input = Some(ModuleInput32 {
            name: name.to_owned(),
            total,
            bytes,
        });
        Ok(())
    }

    pub fn append_module_input(&mut self, offset: u32, bytes: &[u8]) -> Result<(), HostError> {
        self.module_input_guard()?;
        let input = self
            .modules
            .input
            .as_mut()
            .ok_or(HostError::Module(ModuleError32::InvalidRequest))?;
        if bytes.is_empty()
            || bytes.len() > TRANSFER_SIZE
            || offset as usize != input.bytes.len()
            || input
                .bytes
                .len()
                .checked_add(bytes.len())
                .is_none_or(|end| end > input.total as usize)
        {
            return Err(HostError::Module(ModuleError32::InvalidRequest));
        }
        input.bytes.extend_from_slice(bytes);
        Ok(())
    }

    pub fn commit_module_input(&mut self) -> Result<(), HostError> {
        self.module_input_guard()?;
        let input = self
            .modules
            .input
            .as_ref()
            .ok_or(HostError::Module(ModuleError32::InvalidRequest))?;
        if input.bytes.len() != input.total as usize {
            return Err(HostError::Module(ModuleError32::InvalidRequest));
        }
        self.modules
            .catalog
            .add_guest(&input.name, &input.bytes)
            .map_err(HostError::Module)?;
        self.modules.input = None;
        Ok(())
    }

    pub fn abort_module_input(&mut self) -> Result<(), HostError> {
        self.memory()?;
        self.modules.input = None;
        Ok(())
    }

    fn module_input_guard(&self) -> Result<(), HostError> {
        self.memory()?;
        if !self.modules.operations.is_empty()
            || self.modules.source.is_some()
            || self.pending_call.is_some()
            || self.callback.is_some()
        {
            return Err(HostError::Module(ModuleError32::Busy));
        }
        Ok(())
    }
}
