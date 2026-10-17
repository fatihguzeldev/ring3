use crate::windows::user32::User32Error32 as E;
pub(crate) const MAX_ITEMS: usize = 4096;
pub(crate) const MAX_TEXT: usize = 65_535;
const MAX_BYTES: usize = 1_048_576;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct TabItem32 {
    pub text: Vec<u16>,
    pub image: i32,
    pub state: u32,
    pub data: Vec<u8>,
}
impl Default for TabItem32 {
    fn default() -> Self {
        Self {
            text: Vec::new(),
            image: -1,
            state: 0,
            data: Vec::new(),
        }
    }
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Tab32 {
    pub items: Vec<TabItem32>,
    pub selected: i32,
    pub focused: i32,
    pub font: u32,
    pub extra: usize,
    pub unicode: bool,
    pub padding: (i32, i32),
    pub item_size: Option<(i32, i32)>,
    pub min_width: i32,
    pub extended: u32,
}
impl Default for Tab32 {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            selected: -1,
            focused: -1,
            font: 0,
            extra: 4,
            unicode: true,
            padding: (6, 3),
            item_size: None,
            min_width: -1,
            extended: 0,
        }
    }
}
impl Tab32 {
    pub(crate) fn check_item(&self, replaced: Option<usize>, item: &TabItem32) -> Result<(), E> {
        if item.text.len() > MAX_TEXT || item.data.len() != self.extra {
            return Err(E::Win32(87));
        }
        let old = replaced
            .and_then(|index| self.items.get(index))
            .map_or(0, |item| item.text.len() * 2 + item.data.len());
        let total = self
            .items
            .iter()
            .map(|item| item.text.len() * 2 + item.data.len())
            .sum::<usize>();
        if total - old + item.text.len() * 2 + item.data.len() > MAX_BYTES {
            return Err(E::Capacity);
        }
        Ok(())
    }
    pub(crate) fn prepare_insert(&mut self, item: &TabItem32) -> Result<(), E> {
        if self.items.len() == MAX_ITEMS {
            return Err(E::Capacity);
        }
        self.check_item(None, item)?;
        self.items.try_reserve(1).map_err(|_| E::Allocation)
    }
    pub(crate) fn insert(&mut self, index: usize, item: TabItem32) -> usize {
        let first = self.items.is_empty();
        let index = index.min(self.items.len());
        self.items.insert(index, item);
        if first {
            self.selected = 0;
            self.focused = 0;
        } else {
            if self.selected >= 0 && index <= self.selected as usize {
                self.selected += 1;
            }
            if self.focused >= 0 && index <= self.focused as usize {
                self.focused += 1;
            }
        }
        index
    }
    pub(crate) fn delete(&mut self, index: usize) -> bool {
        if index >= self.items.len() {
            return false;
        }
        self.items.remove(index);
        for value in [&mut self.selected, &mut self.focused] {
            if *value == index as i32 {
                *value = -1;
            } else if *value > index as i32 {
                *value -= 1;
            }
        }
        true
    }
    pub(crate) fn select(&mut self, index: i32) -> i32 {
        if index < -1 || index >= self.items.len() as i32 {
            return -1;
        }
        let old = self.selected;
        self.selected = index;
        if index >= 0 {
            self.focused = index;
        }
        old
    }
}
#[cfg(test)]
mod tests;
