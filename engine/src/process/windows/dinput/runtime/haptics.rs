use super::super::{DirectInputError32, objects::ObjectKind32};
use crate::{
    input::dinput::{DeviceKind32, haptics::HapticCapabilities32},
    process::{EngineInstance, HostError},
};
impl EngineInstance {
    pub fn configure_controller_haptics(
        &mut self,
        epoch: u64,
        slot: u8,
        capabilities: HapticCapabilities32,
    ) -> Result<(), HostError> {
        if self.media_contexts()?.input.session_epoch() != epoch {
            return Err(invalid());
        }
        capabilities.validate().map_err(|_| invalid())?;
        let runtime = self
            .dinput
            .as_mut()
            .ok_or(HostError::DirectInput(DirectInputError32::NotConfigured))?;
        let index = usize::from(slot);
        let descriptor = runtime
            .controllers
            .get(index)
            .and_then(Option::as_ref)
            .ok_or_else(invalid)?;
        if capabilities.axes>descriptor.axes||runtime.haptic_capabilities[index].is_some()
            ||runtime.records.iter().flatten().any(|record|matches!(&record.kind,ObjectKind32::Device(device) if device.kind==DeviceKind32::Controller(slot))) {return Err(invalid())}
        runtime.haptic_capabilities[index] = Some(capabilities);
        Ok(())
    }
}
fn invalid() -> HostError {
    HostError::DirectInput(DirectInputError32::InvalidRequest)
}
