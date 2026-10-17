use crate::windows::user32::{Point32, Rect32, User32Error32 as E};
pub(crate) const MAX_ITEMS: usize = 4096;
pub(crate) const MAX_TEXT: usize = 65_535;
const MAX_TEXT_BYTES: usize = 1_048_576;

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct HeaderItem32 {
    pub key: u64,
    pub width: i32,
    pub text: Vec<u16>,
    pub format: u32,
    pub data: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HeaderTrack32 {
    pub key: u64,
    pub divider: bool,
    pub origin: i32,
    pub width: i32,
    pub proposed: i32,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Header32 {
    pub items: Vec<HeaderItem32>,
    pub order: Vec<usize>,
    next_key: u64,
    pub font: u32,
    pub unicode: bool,
    pub bitmap_margin: i32,
    pub track: Option<HeaderTrack32>,
}
impl Default for Header32 {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            order: Vec::new(),
            next_key: 1,
            font: 0,
            unicode: true,
            bitmap_margin: 6,
            track: None,
        }
    }
}
impl Header32 {
    pub(crate) fn check_item(&self, old: Option<usize>, item: &HeaderItem32) -> Result<(), E> {
        if item.format & !0x4707 != 0 {
            return Err(E::Win32(50));
        }
        if item.width < 0 || item.text.len() > MAX_TEXT || item.format & 3 == 3 {
            return Err(E::Win32(87));
        }
        let total: usize = self.items.iter().map(|item| item.text.len() * 2).sum();
        let old_bytes = old
            .and_then(|index| self.items.get(index))
            .map_or(0, |item| item.text.len() * 2);
        if total - old_bytes + item.text.len() * 2 > MAX_TEXT_BYTES {
            return Err(E::Capacity);
        }
        let width: i64 = self
            .items
            .iter()
            .enumerate()
            .filter(|(index, _)| Some(*index) != old)
            .map(|(_, item)| i64::from(item.width))
            .sum();
        if width + i64::from(item.width) > i64::from(i32::MAX) {
            return Err(E::Win32(87));
        }
        Ok(())
    }
    pub(crate) fn prepare_insert(&mut self, item: &HeaderItem32) -> Result<(), E> {
        if self.items.len() == MAX_ITEMS {
            return Err(E::Capacity);
        }
        self.next_key.checked_add(1).ok_or(E::Capacity)?;
        self.check_item(None, item)?;
        self.items.try_reserve(1).map_err(|_| E::Allocation)?;
        self.order.try_reserve(1).map_err(|_| E::Allocation)
    }
    pub(crate) fn insert(
        &mut self,
        index: usize,
        position: usize,
        mut item: HeaderItem32,
    ) -> usize {
        let index = index.min(self.items.len());
        let position = position.min(self.order.len());
        item.key = self.next_key;
        self.next_key += 1;
        self.items.insert(index, item);
        for value in &mut self.order {
            if *value >= index {
                *value += 1;
            }
        }
        self.order.insert(position, index);
        index
    }
    pub(crate) fn delete(&mut self, index: usize) -> bool {
        if index >= self.items.len() {
            return false;
        }
        let old = self.items.remove(index);
        self.order.retain(|&value| value != index);
        for value in &mut self.order {
            if *value > index {
                *value -= 1;
            }
        }
        if self.track.is_some_and(|track| track.key == old.key) {
            self.track = None;
        }
        true
    }
    pub(crate) fn position(&self, index: usize) -> Option<usize> {
        self.order.iter().position(|&value| value == index)
    }
    pub(crate) fn move_item(&mut self, index: usize, position: usize) {
        let old = self
            .position(index)
            .expect("header order retains every live item");
        let item = self.order.remove(old);
        self.order.insert(position.min(self.order.len()), item);
    }
    pub(crate) fn check_order(&self, order: &[usize]) -> Result<(), E> {
        if order.len() != self.items.len() {
            return Err(E::Win32(87));
        }
        for (position, &index) in order.iter().enumerate() {
            if index >= order.len() || order[..position].contains(&index) {
                return Err(E::Win32(87));
            }
        }
        Ok(())
    }
    pub(crate) fn rect(&self, index: usize, height: i32) -> Option<Rect32> {
        let position = self.position(index)?;
        let left = self.order[..position]
            .iter()
            .map(|&index| self.items[index].width)
            .sum();
        Some(Rect32 {
            left,
            top: 0,
            right: left + self.items[index].width,
            bottom: height,
        })
    }
    pub(crate) fn hit(&self, point: Point32, width: i32, height: i32) -> (i32, u32) {
        let outside = if point.x < 0 {
            0x800
        } else if point.x >= width {
            0x400
        } else {
            0
        } | if point.y < 0 {
            0x100
        } else if point.y >= height {
            0x200
        } else {
            0
        };
        if outside != 0 {
            return (-1, outside);
        }
        let mut left = 0i32;
        for &index in &self.order {
            let right = left + self.items[index].width;
            if (point.x - right).abs() <= 3 && self.items[index].format & 0x100 == 0 {
                return (
                    index as i32,
                    if self.items[index].width == 0 { 8 } else { 4 },
                );
            }
            if point.x >= left && point.x < right {
                return (index as i32, 2);
            }
            left = right;
        }
        (-1, 1)
    }
}
#[cfg(test)]
mod tests;
