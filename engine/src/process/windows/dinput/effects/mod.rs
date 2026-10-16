mod calls;
mod creation;
mod decoding;
mod enumeration;
mod parameters;

use super::DirectInputRuntime32;
use crate::{
    input::dinput::{DirectInputResult32 as H, haptics::EffectDevice32},
    memory::GuestAddress,
};

impl DirectInputRuntime32 {
    pub(in crate::process::windows::dinput) fn force_device(
        &self,
        pointer: GuestAddress,
    ) -> Result<&EffectDevice32, H> {
        self.device(pointer)?.haptics.as_ref().ok_or(H::UNSUPPORTED)
    }
    pub(in crate::process::windows::dinput) fn force_device_mut(
        &mut self,
        pointer: GuestAddress,
    ) -> Result<&mut EffectDevice32, H> {
        self.device_mut(pointer)?
            .haptics
            .as_mut()
            .ok_or(H::UNSUPPORTED)
    }
    pub(in crate::process::windows::dinput) fn force_exclusive(
        &self,
        pointer: GuestAddress,
    ) -> Result<bool, H> {
        let device = self.device(pointer)?;
        Ok(
            device.native.acquisition() == crate::input::dinput::device::Acquisition32::Acquired
                && device
                    .native
                    .cooperative()
                    .is_some_and(|cooperative| cooperative.exclusive),
        )
    }
}
