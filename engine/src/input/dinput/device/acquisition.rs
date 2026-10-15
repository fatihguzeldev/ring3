use super::DirectInputDevice32;
use crate::input::{
    dinput::{DeviceKind32, DirectInputResult32},
    source::InputSource32,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Acquisition32 {
    Unacquired,
    Acquired,
    Lost,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CooperativeLevel32 {
    pub window: u32,
    pub exclusive: bool,
    pub foreground: bool,
    pub no_windows_key: bool,
}
impl CooperativeLevel32 {
    pub fn new(window: u32, flags: u32, kind: DeviceKind32) -> Result<Self, DirectInputResult32> {
        if flags & !31 != 0
            || !matches!(flags & 3, 1 | 2)
            || !matches!(flags & 12, 4 | 8)
            || window == 0 && flags & 15 != 10
            || flags & 16 != 0 && kind != DeviceKind32::Keyboard
        {
            return Err(DirectInputResult32::INVALID_PARAM);
        }
        if flags & 9 == 9 && matches!(kind, DeviceKind32::Keyboard | DeviceKind32::Mouse) {
            return Err(DirectInputResult32::UNSUPPORTED);
        }
        Ok(Self {
            window,
            exclusive: flags & 1 != 0,
            foreground: flags & 4 != 0,
            no_windows_key: flags & 16 != 0,
        })
    }
}
impl DirectInputDevice32 {
    pub fn acquisition(&self) -> Acquisition32 {
        self.acquisition
    }
    pub fn cooperative(&self) -> Option<CooperativeLevel32> {
        self.cooperative
    }
    pub fn set_cooperative(
        &mut self,
        level: CooperativeLevel32,
    ) -> Result<(), DirectInputResult32> {
        self.check_unacquired()?;
        self.cooperative = Some(level);
        Ok(())
    }
    pub fn acquire(
        &mut self,
        source: &InputSource32,
        active_window: u32,
    ) -> Result<DirectInputResult32, DirectInputResult32> {
        if self.acquisition == Acquisition32::Acquired {
            return Ok(DirectInputResult32::NO_EFFECT);
        }
        if self.format.as_ref().is_none_or(|format| {
            format
                .bindings()
                .iter()
                .all(|binding| binding.object.is_none())
        }) {
            return Err(DirectInputResult32::INVALID_PARAM);
        }
        let level = self.cooperative.ok_or(DirectInputResult32::INVALID_PARAM)?;
        if level.foreground && (!source.snapshot().focused || active_window != level.window) {
            return Err(DirectInputResult32::OTHER_APP);
        }
        if let DeviceKind32::Controller(slot) = self.description.kind
            && !source.snapshot().controllers[usize::from(slot)].connected
        {
            return Err(DirectInputResult32::INPUT_LOST);
        }
        self.cursor = source.tail();
        self.snapshot = *source.snapshot();
        self.baseline = self.snapshot.relative;
        if let DeviceKind32::Controller(slot) = self.description.kind {
            self.controller_baseline = self.snapshot.controllers[usize::from(slot)].axes;
        }
        self.buffer.clear();
        self.overflow = false;
        self.acquisition = Acquisition32::Acquired;
        Ok(DirectInputResult32::OK)
    }
    pub fn unacquire(&mut self) -> DirectInputResult32 {
        let result = if self.acquisition == Acquisition32::Unacquired {
            DirectInputResult32::NO_EFFECT
        } else {
            DirectInputResult32::OK
        };
        self.acquisition = Acquisition32::Unacquired;
        self.buffer.clear();
        self.overflow = false;
        result
    }
    pub fn check_acquired(&self) -> Result<(), DirectInputResult32> {
        match self.acquisition {
            Acquisition32::Acquired => Ok(()),
            Acquisition32::Lost => Err(DirectInputResult32::INPUT_LOST),
            Acquisition32::Unacquired => Err(DirectInputResult32::NOT_ACQUIRED),
        }
    }
    pub fn check_unacquired(&self) -> Result<(), DirectInputResult32> {
        if self.acquisition == Acquisition32::Acquired {
            Err(DirectInputResult32::ACQUIRED)
        } else {
            Ok(())
        }
    }
}
