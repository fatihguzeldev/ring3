use super::DirectInputDevice32;
use crate::input::{
    dinput::{DeviceKind32, DirectInputResult32, format::DeviceObject32},
    source::SourceSnapshot32,
};
impl DirectInputDevice32 {
    pub fn state_size(&self) -> Result<usize, DirectInputResult32> {
        self.check_acquired()?;
        self.format
            .as_ref()
            .map(|format| format.size as usize)
            .ok_or(DirectInputResult32::NOT_INITIALIZED)
    }
    pub fn read_state(&self, output: &mut [u8]) -> Result<(), DirectInputResult32> {
        if output.len() != self.state_size()? {
            return Err(DirectInputResult32::INVALID_PARAM);
        }
        output.fill(0);
        for binding in self.format.as_ref().unwrap().bindings() {
            let value = binding.object.map_or(binding.default_value, |object| {
                self.value(object, &self.snapshot)
            });
            let offset = binding.offset as usize;
            if binding.width == 1 {
                output[offset] = value as u8;
            } else {
                output[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
        Ok(())
    }
    pub fn commit_state_read(&mut self) {
        self.baseline = self.snapshot.relative;
        if let DeviceKind32::Controller(slot) = self.description.kind {
            self.controller_baseline = self.snapshot.controllers[usize::from(slot)].axes;
        }
    }
    pub(super) fn value(&self, object: DeviceObject32, state: &SourceSnapshot32) -> u32 {
        match (self.description.kind, object) {
            (DeviceKind32::Keyboard, DeviceObject32::Key(index)) => {
                u32::from(state.keys[usize::from(index)]) * 0x80
            }
            (DeviceKind32::Mouse, DeviceObject32::Button(index)) => {
                u32::from(state.buttons[usize::from(index)]) * 0x80
            }
            (DeviceKind32::Mouse, DeviceObject32::Axis { index, .. }) => {
                let value = if self.relative() {
                    match index {
                        0 => state.relative.dx.saturating_sub(self.baseline.dx),
                        1 => state.relative.dy.saturating_sub(self.baseline.dy),
                        _ => state.relative.wheel.saturating_sub(self.baseline.wheel),
                    }
                } else {
                    match index {
                        0 => i64::from(state.cursor.0),
                        1 => i64::from(state.cursor.1),
                        _ => state.relative.wheel,
                    }
                };
                value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32 as u32
            }
            (DeviceKind32::Controller(slot), DeviceObject32::Axis { index, aspect }) => {
                let raw = state.controllers[usize::from(slot)].axes
                    [usize::from(index) + usize::from(aspect) * 8];
                let value = if aspect == 0 {
                    self.axes[usize::from(index)].map(raw)
                } else {
                    raw
                };
                if self.relative() {
                    let old =
                        self.controller_baseline[usize::from(index) + usize::from(aspect) * 8];
                    let old = if aspect == 0 {
                        self.axes[usize::from(index)].map(old)
                    } else {
                        old
                    };
                    value.saturating_sub(old) as u32
                } else {
                    value as u32
                }
            }
            (DeviceKind32::Controller(slot), DeviceObject32::Button(index)) => {
                u32::from(state.controllers[usize::from(slot)].buttons[usize::from(index)]) * 0x80
            }
            (DeviceKind32::Controller(slot), DeviceObject32::Pov(index)) => {
                state.controllers[usize::from(slot)].pov[usize::from(index)]
            }
            _ => 0,
        }
    }
}
