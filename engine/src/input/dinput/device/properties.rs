use super::DirectInputDevice32;
use crate::input::dinput::DirectInputResult32;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AxisProperties32 {
    pub minimum: i32,
    pub maximum: i32,
    pub deadzone: u32,
    pub saturation: u32,
    pub raw: bool,
}
impl Default for AxisProperties32 {
    fn default() -> Self {
        Self {
            minimum: 0,
            maximum: 65535,
            deadzone: 0,
            saturation: 10000,
            raw: false,
        }
    }
}
impl AxisProperties32 {
    pub fn map(self, value: i32) -> i32 {
        if self.raw {
            return value;
        }
        let value = i64::from(value.clamp(-32768, 32767));
        let magnitude = if value < 0 {
            -value * 10000 / 32768
        } else {
            value * 10000 / 32767
        };
        let dead = i64::from(self.deadzone);
        let saturation = i64::from(self.saturation).max(dead);
        let normalized = if magnitude <= dead {
            0
        } else if saturation == dead || magnitude >= saturation {
            10000
        } else {
            (magnitude - dead) * 10000 / (saturation - dead)
        };
        let low = i64::from(self.minimum);
        let high = i64::from(self.maximum);
        let center = (low + high + 1) / 2;
        if value < 0 {
            (center - (center - low) * normalized / 10000) as i32
        } else {
            (center + (high - center) * normalized / 10000) as i32
        }
    }
}
impl DirectInputDevice32 {
    pub fn requested_buffer(&self) -> u32 {
        self.requested_buffer
    }
    pub fn set_buffer_size(&mut self, count: u32) -> Result<(), DirectInputResult32> {
        self.check_unacquired()?;
        self.requested_buffer = count;
        self.buffer.clear();
        self.overflow = false;
        Ok(())
    }
    pub fn set_axis_mode(&mut self, relative: bool) -> Result<(), DirectInputResult32> {
        self.check_unacquired()?;
        self.axis_relative = Some(relative);
        Ok(())
    }
    pub fn gain(&self) -> u32 {
        self.gain
    }
    pub fn set_gain(&mut self, gain: u32) -> Result<(), DirectInputResult32> {
        if gain > 10000 {
            return Err(DirectInputResult32::INVALID_PARAM);
        }
        self.gain = gain;
        Ok(())
    }
    pub fn axis(&self, index: u8) -> Result<AxisProperties32, DirectInputResult32> {
        if index >= self.description.axes {
            Err(DirectInputResult32::OBJECT_NOT_FOUND)
        } else {
            Ok(self.axes[usize::from(index)])
        }
    }
    pub fn set_axis(
        &mut self,
        index: Option<u8>,
        properties: AxisProperties32,
    ) -> Result<(), DirectInputResult32> {
        self.check_unacquired()?;
        if properties.minimum >= properties.maximum
            || properties.deadzone > 10000
            || properties.saturation > 10000
        {
            return Err(DirectInputResult32::INVALID_PARAM);
        }
        match index {
            Some(index) if index >= self.description.axes => {
                return Err(DirectInputResult32::OBJECT_NOT_FOUND);
            }
            Some(index) => self.axes[usize::from(index)] = properties,
            None => self.axes[..usize::from(self.description.axes)].fill(properties),
        }
        Ok(())
    }
}
