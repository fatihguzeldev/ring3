use crate::windows::user32::User32Error32;
#[derive(Default)]
pub(crate) struct ControlState32 {
    pub font: u32,
    pub check: u32,
    pub pressed: bool,
    pub selection: (usize, usize),
    pub modified: bool,
    pub limit: usize,
    pub items: Vec<Vec<u16>>,
    pub selected: Option<usize>,
    pub item_data: Vec<u32>,
    pub scroll: (i32, i32, i32, u32),
}
impl ControlState32 {
    pub(crate) fn insert(&mut self, index: usize, value: Vec<u16>) -> Result<usize, User32Error32> {
        if self.items.len() >= 16_384 || index > self.items.len() {
            return Err(User32Error32::Capacity);
        }
        self.items
            .try_reserve(1)
            .map_err(|_| User32Error32::Allocation)?;
        self.item_data
            .try_reserve(1)
            .map_err(|_| User32Error32::Allocation)?;
        self.items.insert(index, value);
        self.item_data.insert(index, 0);
        if let Some(selected) = self.selected.as_mut() {
            if *selected >= index {
                *selected += 1;
            }
        }
        Ok(index)
    }
    pub(crate) fn remove(&mut self, index: usize) -> Result<usize, User32Error32> {
        if index >= self.items.len() {
            return Err(User32Error32::Win32(87));
        }
        self.items.remove(index);
        self.item_data.remove(index);
        self.selected = match self.selected {
            Some(selected) if selected == index => None,
            Some(selected) if selected > index => Some(selected - 1),
            value => value,
        };
        Ok(self.items.len())
    }
}
