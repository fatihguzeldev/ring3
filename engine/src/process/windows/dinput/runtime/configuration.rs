use super::super::{DirectInputConfig32, DirectInputError32, DirectInputRuntime32};
use crate::{
    input::dinput::ControllerDescriptor32,
    process::{CallError, EngineInstance, HostError},
};
impl EngineInstance {
    pub fn configure_dinput(&mut self, config: DirectInputConfig32) -> Result<(), HostError> {
        self.memory()?;
        if self.dinput.is_some() {
            return Err(HostError::DirectInput(
                DirectInputError32::AlreadyConfigured,
            ));
        }
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        self.dinput = Some(DirectInputRuntime32::new(config).map_err(HostError::DirectInput)?);
        Ok(())
    }
    pub fn register_dinput_module(&mut self, base: u32) -> Result<(), HostError> {
        self.register_system_module(crate::input::dinput::api::system_module(base, false))
    }
    pub fn register_dinput8_module(&mut self, base: u32) -> Result<(), HostError> {
        self.register_system_module(crate::input::dinput::api::system_module(base, true))
    }
    pub fn register_input_controller(
        &mut self,
        slot: u8,
        descriptor: ControllerDescriptor32,
    ) -> Result<(), HostError> {
        self.memory()?;
        descriptor
            .validate()
            .map_err(|_| HostError::DirectInput(DirectInputError32::InvalidRequest))?;
        let runtime = self
            .dinput
            .as_mut()
            .ok_or(HostError::DirectInput(DirectInputError32::NotConfigured))?;
        let index = usize::from(slot);
        if index >= runtime.controllers.len()
            || runtime.controllers[index].is_some()
            || runtime
                .controllers
                .iter()
                .flatten()
                .any(|existing| existing.instance == descriptor.instance)
        {
            return Err(HostError::DirectInput(DirectInputError32::InvalidRequest));
        }
        runtime.controllers[index] = Some(descriptor);
        Ok(())
    }
}
