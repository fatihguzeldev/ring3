use crate::windows::{user32::User32Error32 as E, window::WindowHandle32};
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct UpDown32 {
    pub minimum: i32,
    pub maximum: i32,
    pub position: i32,
    pub base: u32,
    pub font: u32,
    pub unicode: bool,
    pub buddy: Option<WindowHandle32>,
    pub acceleration: Vec<(u32, u32)>,
    pub pressed: Option<(i32, u64)>,
    pub key_active: bool,
}
impl Default for UpDown32 {
    fn default() -> Self {
        Self {
            minimum: 0,
            maximum: 100,
            position: 0,
            base: 10,
            font: 0,
            unicode: true,
            buddy: None,
            acceleration: Vec::new(),
            pressed: None,
            key_active: false,
        }
    }
}
impl UpDown32 {
    pub(crate) fn clamp(&self, value: i64) -> i32 {
        value.clamp(
            i64::from(self.minimum.min(self.maximum)),
            i64::from(self.minimum.max(self.maximum)),
        ) as i32
    }
    pub(crate) fn delta(&self, start: i32, delta: i32, wrap: bool) -> i32 {
        let direction = if self.maximum >= self.minimum {
            1i64
        } else {
            -1
        };
        let value = i64::from(start) + i64::from(delta) * direction;
        let min = i64::from(self.minimum.min(self.maximum));
        let max = i64::from(self.minimum.max(self.maximum));
        if wrap {
            (min + (value - min).rem_euclid(max - min + 1)) as i32
        } else {
            self.clamp(value)
        }
    }
    pub(crate) fn set_range(&mut self, minimum: i32, maximum: i32) {
        self.minimum = minimum;
        self.maximum = maximum;
        self.position = self.clamp(i64::from(self.position));
    }
    pub(crate) fn check_acceleration(values: &[(u32, u32)]) -> Result<(), E> {
        if values.is_empty() || values.len() > 64 {
            return Err(E::Win32(87));
        }
        let mut last = 0;
        for (index, &(seconds, increment)) in values.iter().enumerate() {
            if increment == 0 || increment > i32::MAX as u32 || (index != 0 && seconds < last) {
                return Err(E::Win32(87));
            }
            last = seconds;
        }
        Ok(())
    }
    pub(crate) fn acceleration(&self) -> &[(u32, u32)] {
        if self.acceleration.is_empty() {
            &[(0, 1)]
        } else {
            &self.acceleration
        }
    }
    pub(crate) fn step(&self, seconds: u64) -> i32 {
        self.acceleration()
            .iter()
            .rev()
            .find(|&&(start, _)| u64::from(start) <= seconds)
            .map_or(1, |&(_, increment)| increment as i32)
    }
    pub(crate) fn format(&self, position: i32, thousands: bool) -> Vec<u16> {
        Self::format_value(position, self.base, thousands)
    }
    pub(crate) fn format_value(position: i32, base: u32, thousands: bool) -> Vec<u16> {
        let raw = if base == 16 {
            format!("{:04X}", position as u32)
        } else {
            position.to_string()
        };
        if !thousands || base == 16 {
            return raw.encode_utf16().collect();
        }
        let mut result = Vec::new();
        for (index, byte) in raw.bytes().enumerate() {
            if byte != b'-'
                && index > usize::from(raw.starts_with('-'))
                && (raw.len() - index) % 3 == 0
            {
                result.push(u16::from(b','));
            }
            result.push(u16::from(byte));
        }
        result
    }
    pub(crate) fn parse(&self, text: &[u16]) -> Option<i32> {
        let mut raw = String::new();
        for &unit in text {
            let byte = u8::try_from(unit).ok()?;
            if byte != b',' {
                raw.push(char::from(byte));
            }
        }
        let value = if self.base == 16 {
            u32::from_str_radix(
                raw.trim().trim_start_matches("0x").trim_start_matches("0X"),
                16,
            )
            .ok()? as i32
        } else {
            raw.trim().parse().ok()?
        };
        (value == self.clamp(i64::from(value))).then_some(value)
    }
}
#[cfg(test)]
mod tests;
