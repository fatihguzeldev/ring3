use crate::windows::user32::{Point32, Rect32, User32Error32 as E};
pub(crate) const MAX_BUTTONS: usize = 4096;
const MAX_TEXT: usize = 1 << 20;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ToolbarButton32 {
    pub key: u64,
    pub bitmap: i32,
    pub command: u32,
    pub state: u8,
    pub style: u8,
    pub data: u32,
    pub string: u32,
    pub text: Vec<u16>,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Toolbar32 {
    pub buttons: Vec<ToolbarButton32>,
    pub strings: Vec<Vec<u16>>,
    pub font: u32,
    pub size: (i32, i32),
    pub pressed: Option<u64>,
    pub unicode: bool,
    next: u64,
}
impl Default for Toolbar32 {
    fn default() -> Self {
        Self {
            buttons: Vec::new(),
            strings: Vec::new(),
            font: 0,
            size: (24, 22),
            pressed: None,
            unicode: false,
            next: 1,
        }
    }
}
impl Toolbar32 {
    #[cfg(test)]
    pub fn insert(&mut self, index: usize, buttons: Vec<ToolbarButton32>) -> Result<(), E> {
        self.insert_with_strings(index, buttons, Vec::new())
    }
    pub fn insert_with_strings(
        &mut self,
        index: usize,
        mut buttons: Vec<ToolbarButton32>,
        strings: Vec<Vec<u16>>,
    ) -> Result<(), E> {
        let count = self
            .buttons
            .len()
            .checked_add(buttons.len())
            .filter(|n| *n <= MAX_BUTTONS)
            .ok_or(E::Capacity)?;
        let next = self
            .next
            .checked_add(buttons.len() as u64)
            .ok_or(E::Capacity)?;
        let mut units = 0usize;
        for button in self.buttons.iter().chain(&buttons) {
            units = units
                .checked_add(button.text.len())
                .filter(|n| *n <= MAX_TEXT)
                .ok_or(E::Capacity)?;
            if button.style & (8 | 0x10 | 0x80) != 0
                || button.state & 0x20 != 0
                || button.style & 1 != 0 && button.bitmap > 65535
                || button.style & 1 == 0 && button.bitmap != -2
            {
                return Err(E::Win32(50));
            }
        }
        self.buttons
            .try_reserve(count - self.buttons.len())
            .map_err(|_| E::Allocation)?;
        self.check_strings(&strings)?;
        self.strings
            .try_reserve(strings.len())
            .map_err(|_| E::Allocation)?;
        for (offset, button) in buttons.iter_mut().enumerate() {
            button.key = self.next + offset as u64;
        }
        let index = index.min(self.buttons.len());
        self.buttons.splice(index..index, buttons);
        self.strings.extend(strings);
        self.next = next;
        Ok(())
    }
    pub fn command(&self, id: u32) -> Option<usize> {
        self.buttons
            .iter()
            .position(|b| b.command == id && b.style & 1 == 0)
    }
    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.buttons.len() {
            return false;
        }
        let removed = self.buttons.remove(index);
        if self.pressed == Some(removed.key) {
            self.pressed = None;
        }
        true
    }
    pub fn layout(&self) -> impl Iterator<Item = (usize, Rect32)> + '_ {
        let mut left = 0i32;
        self.buttons.iter().enumerate().filter_map(move |(i, b)| {
            if b.state & 8 != 0 {
                return None;
            }
            let width = if b.style & 1 != 0 {
                if b.bitmap > 0 { b.bitmap } else { 8 }
            } else {
                self.size.0
            };
            let right = left.saturating_add(width);
            let rect = Rect32 {
                left,
                top: 0,
                right,
                bottom: self.size.1,
            };
            left = right;
            Some((i, rect))
        })
    }
    pub fn rect(&self, index: usize) -> Option<Rect32> {
        self.layout().find_map(|(i, r)| (i == index).then_some(r))
    }
    pub fn hit(&self, point: Point32) -> Option<usize> {
        self.layout().find_map(|(i, r)| {
            (point.x >= r.left && point.x < r.right && point.y >= r.top && point.y < r.bottom)
                .then_some(i)
        })
    }
    pub fn check_button(&mut self, index: usize, checked: bool) {
        if checked && self.buttons[index].style & 4 != 0 {
            let mut start = index;
            while start > 0 && self.buttons[start - 1].style & 4 != 0 {
                start -= 1;
            }
            let mut end = index + 1;
            while end < self.buttons.len() && self.buttons[end].style & 4 != 0 {
                end += 1;
            }
            for b in &mut self.buttons[start..end] {
                b.state &= !1;
            }
        }
        self.buttons[index].state = self.buttons[index].state & !1 | u8::from(checked);
    }
    pub fn add_strings(&mut self, strings: Vec<Vec<u16>>) -> Result<usize, E> {
        self.check_strings(&strings)?;
        self.strings
            .try_reserve(strings.len())
            .map_err(|_| E::Allocation)?;
        let first = self.strings.len();
        self.strings.extend(strings);
        Ok(first)
    }
    fn check_strings(&self, strings: &[Vec<u16>]) -> Result<(), E> {
        if self
            .strings
            .len()
            .checked_add(strings.len())
            .is_none_or(|n| n > MAX_BUTTONS)
        {
            return Err(E::Capacity);
        }
        self.strings
            .iter()
            .chain(strings)
            .try_fold(0usize, |n, s| n.checked_add(s.len()))
            .filter(|n| *n <= MAX_TEXT)
            .ok_or(E::Capacity)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
