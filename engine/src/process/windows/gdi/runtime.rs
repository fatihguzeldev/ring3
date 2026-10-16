use super::{GdiConfig32, GdiFrame32, GdiRuntime32};
use crate::{
    process::{EngineInstance, HostError},
    windows::gdi::GdiError32,
};

impl EngineInstance {
    pub fn configure_gdi(&mut self, config: GdiConfig32) -> Result<(), HostError> {
        self.memory()?;
        if self.gdi.is_some()
            || self.image_started
            || self.pending_call.is_some()
            || self.callback.is_some()
        {
            return Err(HostError::Gdi(GdiError32::InvalidRequest));
        }
        let runtime = GdiRuntime32::new(config).map_err(HostError::Gdi)?;
        let bounds = [runtime.bounds];
        self.check_guest_reservation_admission(&bounds)?;
        self.gdi = Some(runtime);
        Ok(())
    }
    pub fn register_gdi32_module(&mut self, base: u32) -> Result<(), HostError> {
        self.register_system_module(crate::windows::gdi::gdi32_module(base))
    }
    pub fn pending_gdi_frame(&self, index: u32) -> Result<Option<GdiFrame32>, HostError> {
        self.memory()?;
        Ok(self
            .gdi
            .as_ref()
            .and_then(|runtime| runtime.frame(index as usize)))
    }
    pub fn read_gdi_frame(
        &self,
        id: u64,
        revision: u64,
        offset: u32,
        length: u32,
    ) -> Result<&[u8], HostError> {
        self.memory()?;
        self.gdi
            .as_ref()
            .ok_or(HostError::InvalidRequest)?
            .read_frame(id, revision, offset, length)
            .map_err(HostError::Gdi)
    }
    pub fn acknowledge_gdi_frame(&mut self, id: u64, revision: u64) -> Result<(), HostError> {
        self.memory()?;
        self.gdi
            .as_mut()
            .ok_or(HostError::InvalidRequest)?
            .acknowledge_frame(id, revision)
            .map_err(HostError::Gdi)
    }
}
