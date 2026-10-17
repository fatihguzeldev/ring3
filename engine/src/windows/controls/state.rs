use crate::windows::user32::User32Error32;
#[derive(Default)]
pub(crate) struct ControlState32 {
    pub image: Option<super::ControlImage32>,
    pub image_owned: bool,
    pub font: u32,
    pub check: u32,
    pub pressed: bool,
    pub mouse_down: bool,
    pub key_down: bool,
    pub item_height: i32,
    pub drop_height: u32,
    pub dropped: bool,
    pub opening: bool,
    pub closing: bool,
    pub drop_selection: Option<usize>,
    pub popup: Option<crate::windows::window::WindowHandle32>,
    pub list_owner: Option<crate::windows::window::WindowHandle32>,
    pub selection: (usize, usize),
    pub modified: bool,
    pub limit: usize,
    pub items: Vec<Vec<u16>>,
    pub selected: Option<usize>,
    pub selected_items: Vec<bool>,
    pub anchor: Option<usize>,
    pub drag_select: bool,
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
        self.selected_items
            .try_reserve(1)
            .map_err(|_| User32Error32::Allocation)?;
        self.items.insert(index, value);
        self.selected_items.insert(index, false);
        self.item_data.insert(index, 0);
        if let Some(selected) = self.selected.as_mut() {
            if *selected >= index {
                *selected += 1;
            }
        }
        if let Some(anchor) = self.anchor.as_mut() {
            if *anchor >= index {
                *anchor += 1;
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
        self.selected_items.remove(index);
        self.anchor = match self.anchor {
            Some(anchor) if anchor == index => None,
            Some(anchor) if anchor > index => Some(anchor - 1),
            value => value,
        };
        self.selected = match self.selected {
            Some(selected) if selected == index => None,
            Some(selected) if selected > index => Some(selected - 1),
            value => value,
        };
        Ok(self.items.len())
    }
}
