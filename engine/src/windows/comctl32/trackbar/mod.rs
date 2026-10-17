use crate::windows::user32::{Rect32, User32Error32 as E};
const MAX_TICKS: usize = 4096;
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Trackbar32 {
    pub minimum: i32,
    pub maximum: i32,
    pub position: i32,
    pub line: i32,
    pub page: Option<i32>,
    pub selection: (i32, i32),
    pub thumb_length: i32,
    pub font: u32,
    pub unicode: bool,
    pub ticks: Vec<i32>,
    pub frequency: Option<u32>,
    pub dragging: bool,
    pub pressed: Option<(i32, i32)>,
    pub key_active: bool,
}
impl Default for Trackbar32 {
    fn default() -> Self {
        Self {
            minimum: 0,
            maximum: 100,
            position: 0,
            line: 1,
            page: None,
            selection: (0, 0),
            thumb_length: 20,
            font: 0,
            unicode: true,
            ticks: Vec::new(),
            frequency: None,
            dragging: false,
            pressed: None,
            key_active: false,
        }
    }
}
impl Trackbar32 {
    pub(crate) fn clamp(&self, value: i64) -> i32 {
        value.clamp(i64::from(self.minimum), i64::from(self.maximum)) as i32
    }
    pub(crate) fn page(&self) -> i32 {
        self.page
            .unwrap_or(((i64::from(self.maximum) - i64::from(self.minimum)) / 5).max(1) as i32)
    }
    pub(crate) fn set_range(&mut self, minimum: i32, maximum: i32) -> Result<(), E> {
        if minimum > maximum {
            return Err(E::Win32(87));
        }
        self.check_frequency(minimum, maximum, self.frequency)?;
        self.minimum = minimum;
        self.maximum = maximum;
        self.position = self.clamp(i64::from(self.position));
        self.selection = (
            self.clamp(i64::from(self.selection.0)),
            self.clamp(i64::from(self.selection.1)),
        );
        self.ticks.retain(|&tick| tick > minimum && tick < maximum);
        Ok(())
    }
    fn check_frequency(&self, min: i32, max: i32, frequency: Option<u32>) -> Result<(), E> {
        let count = frequency.map_or(0, |frequency| {
            ((i64::from(max) - i64::from(min) - 1).max(0) as u64) / u64::from(frequency)
        });
        if count > MAX_TICKS as u64 {
            return Err(E::Capacity);
        }
        Ok(())
    }
    pub(crate) fn set_frequency(&mut self, frequency: u32) -> Result<(), E> {
        let frequency = if frequency == 0 {
            None
        } else {
            Some(frequency)
        };
        self.check_frequency(self.minimum, self.maximum, frequency)?;
        self.frequency = frequency;
        self.ticks.clear();
        Ok(())
    }
    pub(crate) fn tick_count(&self) -> usize {
        self.frequency.map_or(self.ticks.len(), |frequency| {
            ((i64::from(self.maximum) - i64::from(self.minimum) - 1).max(0) as u64
                / u64::from(frequency)) as usize
        })
    }
    pub(crate) fn tick(&self, index: usize) -> Option<i32> {
        if index >= self.tick_count() {
            return None;
        }
        self.frequency.map_or_else(
            || self.ticks.get(index).copied(),
            |frequency| {
                Some((i64::from(self.minimum) + (index as i64 + 1) * i64::from(frequency)) as i32)
            },
        )
    }
    pub(crate) fn add_tick(&mut self, value: i32) -> Result<bool, E> {
        if value <= self.minimum || value >= self.maximum {
            return Ok(false);
        }
        if self.frequency.is_some() {
            return Err(E::Win32(50));
        }
        if self.ticks.contains(&value) {
            return Ok(true);
        }
        if self.ticks.len() == MAX_TICKS {
            return Err(E::Capacity);
        }
        self.ticks.try_reserve(1).map_err(|_| E::Allocation)?;
        let index = self.ticks.partition_point(|&tick| tick < value);
        self.ticks.insert(index, value);
        Ok(true)
    }
    pub(crate) fn channel(&self, width: i32, height: i32, vertical: bool) -> Rect32 {
        let (length, cross) = if vertical {
            (height, width)
        } else {
            (width, height)
        };
        let inset = (self.thumb_length / 2 + 2).min(length / 2).max(0);
        let center = cross / 2;
        let axis = Rect32 {
            left: inset,
            top: (center - 2).max(0),
            right: (length - inset).max(inset),
            bottom: (center + 2).min(cross),
        };
        if vertical {
            Rect32 {
                left: axis.top,
                top: axis.left,
                right: axis.bottom,
                bottom: axis.right,
            }
        } else {
            axis
        }
    }
    pub(crate) fn coordinate(&self, value: i32, channel: Rect32, vertical: bool) -> i32 {
        let (first, last) = if vertical {
            (channel.top, channel.bottom)
        } else {
            (channel.left, channel.right)
        };
        let span = i64::from(self.maximum) - i64::from(self.minimum);
        if span == 0 {
            return first;
        }
        (i64::from(first)
            + (i64::from(self.clamp(i64::from(value))) - i64::from(self.minimum))
                * i64::from(last - first)
                / span) as i32
    }
    pub(crate) fn value_at(&self, coordinate: i32, channel: Rect32, vertical: bool) -> i32 {
        let (first, last) = if vertical {
            (channel.top, channel.bottom)
        } else {
            (channel.left, channel.right)
        };
        if last <= first {
            return self.minimum;
        }
        let offset = i64::from(coordinate.clamp(first, last) - first);
        let span = i64::from(self.maximum) - i64::from(self.minimum);
        self.clamp(
            i64::from(self.minimum)
                + (offset * span + i64::from(last - first) / 2) / i64::from(last - first),
        )
    }
    pub(crate) fn thumb(&self, width: i32, height: i32, vertical: bool) -> Rect32 {
        let channel = self.channel(width, height, vertical);
        let center = self.coordinate(self.position, channel, vertical);
        let cross = if vertical { width } else { height };
        let half = (self.thumb_length / 2).min(cross / 2).max(1);
        let axis = Rect32 {
            left: center - 4,
            top: (cross / 2 - half).max(0),
            right: center + 5,
            bottom: (cross / 2 + half).min(cross),
        };
        if vertical {
            Rect32 {
                left: axis.top,
                top: axis.left,
                right: axis.bottom,
                bottom: axis.right,
            }
        } else {
            axis
        }
    }
}
#[cfg(test)]
mod tests;
