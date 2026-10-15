use super::{Device, DeviceError};

impl Device {
    pub fn validate_clear(&self, flags: u32, depth: u32) -> Result<(), DeviceError> {
        use super::super::resources::Format;
        self.available()?;
        if flags == 0 || flags & !7 != 0 || (flags & 6 != 0 && self.depth_target.is_none()) {
            return Err(DeviceError::InvalidCall);
        }
        if flags & 2 != 0 && !(0.0..=1.0).contains(&f32::from_bits(depth)) {
            return Err(DeviceError::InvalidCall);
        }
        if flags & 4 != 0
            && !matches!(
                self.resources
                    .desc(self.depth_target.unwrap().resource)?
                    .format(),
                Format::D15S1 | Format::D24S8 | Format::D24X4S4
            )
        {
            return Err(DeviceError::InvalidCall);
        }
        Ok(())
    }
    pub fn validate_device(&self) -> Result<u32, DeviceError> {
        self.available()?;
        if self.state.snapshot().render[137] != 0
            && self
                .vertex
                .as_ref()
                .is_none_or(|shader| shader.function.is_none())
            && self
                .state
                .snapshot()
                .lights
                .iter()
                .filter(|entry| entry.enabled)
                .count()
                > super::super::state::MAX_ACTIVE_LIGHTS
        {
            return Err(DeviceError::NotAvailable);
        }
        if self.pixel.is_some() {
            return Ok(1);
        }
        let stages = &self.state.snapshot().texture_stage;
        for (index, stage) in stages.iter().enumerate() {
            let color = stage[1];
            if color == 1 {
                break;
            }
            if !(2..=26).contains(&color) || !(2..=26).contains(&stage[4]) {
                return Err(DeviceError::InvalidCall);
            }
            for argument in [stage[2], stage[3], stage[5], stage[6], stage[26], stage[27]] {
                if argument & !0x3f != 0 || argument & 15 > 5 {
                    return Err(DeviceError::InvalidCall);
                }
            }
            if !matches!(stage[28], 1 | 5)
                || (matches!(color, 22 | 23)
                    && (index + 1 == stages.len() || stages[index + 1][1] == 1))
            {
                return Err(DeviceError::InvalidCall);
            }
            if stage[11] & 0xffff > 7
                || stage[11] >> 16 > 3
                || stage[24] & !0x10f != 0
                || stage[24] & 15 > 4
                || (stage[24] & 0x100 != 0 && stage[24] & 15 < 2)
            {
                return Err(DeviceError::InvalidCall);
            }
            if !matches!(stage[13], 1..=5)
                || !matches!(stage[14], 1..=5)
                || !matches!(stage[25], 1..=5)
                || !matches!(stage[16], 1..=5)
                || !matches!(stage[17], 1..=5)
                || !matches!(stage[18], 0..=2)
                || stage[21] == 0
                || stage[21] > 16
            {
                return Err(DeviceError::InvalidCall);
            }
        }
        Ok(1)
    }
}
