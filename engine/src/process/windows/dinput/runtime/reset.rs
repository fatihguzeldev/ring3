use super::super::objects::ObjectKind32;
use crate::{
    input::dinput::device::Acquisition32,
    process::{EngineInstance, HostError},
    windows::synchronization::EventBinding32,
};
pub struct PreparedDirectInputReset32 {
    notifications: Vec<(EventBinding32, u64)>,
}
impl PreparedDirectInputReset32 {
    pub(in crate::process) fn notifications(&self) -> &[(EventBinding32, u64)] {
        &self.notifications
    }
}
impl EngineInstance {
    pub(in crate::process) fn prepare_dinput_media_reset(
        &self,
    ) -> Result<PreparedDirectInputReset32, HostError> {
        let Some(runtime) = self.dinput.as_ref() else {
            return Ok(PreparedDirectInputReset32 {
                notifications: Vec::new(),
            });
        };
        let mut notifications = Vec::new();
        notifications
            .try_reserve_exact(runtime.config.max_devices as usize)
            .map_err(|_| HostError::DirectInput(super::super::DirectInputError32::Allocation))?;
        for record in runtime.records.iter().flatten() {
            if let ObjectKind32::Device(device) = &record.kind
                && device.native.acquisition() == Acquisition32::Acquired
                && let Some(binding) = device.notification
            {
                notifications.push((binding, 1));
            }
        }
        Ok(PreparedDirectInputReset32 { notifications })
    }
    pub(in crate::process) fn commit_dinput_media_reset(
        &mut self,
        _prepared: PreparedDirectInputReset32,
    ) {
        self.reset_dinput_input();
    }
}
