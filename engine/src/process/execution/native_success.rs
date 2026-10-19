#![forbid(unsafe_code)]

use crate::{
    cpu::dbt::{CompileError, RegistryError},
    process::{CallError, EngineInstance, HostError, cpu::events::detection::DetectionSlot32},
};

const MODULE_LIMIT: usize = 512 * 1024;

impl EngineInstance {
    // opt-in sibling for captured native helpers, unshared memory and pure clocks.
    // compiled receipts enforce lifecycle, not import provenance; callers must
    // exclude reentrant processing and raw architectural writes during execution.
    // successful requests replace this buffer; failed requests preserve it.
    pub(in crate::process) fn prepare_native_success_module(
        &mut self,
        key: u64,
        id: u64,
    ) -> Result<&[u8], HostError> {
        if self.execution_frame.is_some()
            || matches!(
                self.exception_detection.as_ref().map(|slot| &slot[0]),
                Some(DetectionSlot32::Active { .. })
            )
        {
            return Err(HostError::Call(CallError::Busy));
        }
        self.guard_detection_material_change()?;
        self.guard_resident(key, id)?;
        let region = self.guard_resident_unit(key, id)?;
        self.check_decode_key(region)?;
        let bytes = region
            .emit_native_success(key, id)
            .map_err(|error| HostError::Resident(RegistryError::Compile(error)))?;
        if bytes.len() > MODULE_LIMIT {
            return Err(HostError::Resident(RegistryError::Compile(
                CompileError::WasmLimit,
            )));
        }
        self.native_success_module = bytes;
        Ok(&self.native_success_module)
    }
}

#[cfg(test)]
mod tests;
