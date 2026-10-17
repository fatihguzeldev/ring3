#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Progress32 {
    pub minimum: i32,
    pub maximum: i32,
    pub position: i32,
    pub step: i32,
    pub bar: u32,
    pub background: u32,
    pub font: u32,
}

impl Default for Progress32 {
    fn default() -> Self {
        Self {
            minimum: 0,
            maximum: 100,
            position: 0,
            step: 10,
            bar: 0xff00_0000,
            background: 0xff00_0000,
            font: 0,
        }
    }
}

impl Progress32 {
    pub(crate) fn set_position(&mut self, position: i32) -> u32 {
        let old = self.position;
        self.position = position.max(self.minimum).min(self.maximum);
        old as u32
    }

    pub(crate) fn set_range(&mut self, minimum: i32, maximum: i32) -> u32 {
        let old = (self.minimum as u32 & 0xffff) | (self.maximum as u32) << 16;
        self.minimum = minimum;
        self.maximum = maximum;
        self.position = self.position.max(minimum).min(maximum);
        old
    }

    pub(crate) fn set_step(&mut self, step: i32) -> u32 {
        let old = self.step;
        self.step = step;
        old as u32
    }

    pub(crate) fn delta(&mut self, delta: i32) -> u32 {
        self.set_position(self.position.wrapping_add(delta))
    }

    pub(crate) fn advance(&mut self) -> u32 {
        let old = self.position;
        let position = self.position.wrapping_add(self.step);
        let span = i64::from(self.maximum) - i64::from(self.minimum);
        self.position = if span > 0 && position > self.maximum {
            (i64::from(self.minimum) + (i64::from(position) - i64::from(self.minimum)) % span)
                as i32
        } else if span > 0 && position < self.minimum {
            (i64::from(self.maximum) - (i64::from(self.minimum) - i64::from(position)) % span)
                as i32
        } else {
            position.max(self.minimum).min(self.maximum)
        };
        old as u32
    }
}

#[cfg(test)]
mod tests;
