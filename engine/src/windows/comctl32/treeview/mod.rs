use crate::windows::user32::User32Error32 as E;
pub(crate) const MAX_ITEMS: usize = 4096;
pub(crate) const MAX_TEXT: usize = 65535;
const MAX_TEXT_UNITS: usize = 524288;
pub(crate) const ROOT: u32 = 0xffff0000;
pub(crate) const FIRST: u32 = 0xffff0001;
pub(crate) const LAST: u32 = 0xffff0002;
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Item32 {
    pub id: u32,
    pub parent: u32,
    pub revision: u64,
    pub text: Vec<u16>,
    pub state: u32,
    pub image: i32,
    pub selected_image: i32,
    pub children: i32,
    pub data: u32,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct TreeView32 {
    pub items: Vec<Item32>,
    pub selected: u32,
    pub drop_target: u32,
    pub first: u32,
    pub font: u32,
    pub indent: i32,
    pub item_height: i32,
    pub unicode: bool,
    pub background: u32,
    pub text_color: u32,
}
impl Default for TreeView32 {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            selected: 0,
            drop_target: 0,
            first: 0,
            font: 0,
            indent: 19,
            item_height: 0,
            unicode: true,
            background: 0xff000000,
            text_color: 0xff000000,
        }
    }
}
impl TreeView32 {
    pub(crate) fn item(&self, id: u32) -> Option<&Item32> {
        self.items.iter().find(|item| item.id == id)
    }
    pub(crate) fn item_mut(&mut self, id: u32) -> Option<&mut Item32> {
        self.items.iter_mut().find(|item| item.id == id)
    }
    pub(crate) fn descendant(&self, id: u32, root: u32) -> bool {
        if root == ROOT || root == 0 {
            return self.item(id).is_some();
        }
        let mut current = id;
        for _ in 0..=self.items.len() {
            if current == root {
                return true;
            }
            let Some(item) = self.item(current) else {
                return false;
            };
            if item.parent == 0 {
                return false;
            }
            current = item.parent;
        }
        false
    }
    pub(crate) fn depth(&self, id: u32) -> usize {
        let mut current = id;
        let mut depth = 0;
        while let Some(item) = self.item(current) {
            if item.parent == 0 {
                break;
            }
            depth += 1;
            current = item.parent;
        }
        depth
    }
    pub(crate) fn visible(&self, id: u32) -> bool {
        let mut current = id;
        while let Some(item) = self.item(current) {
            if item.parent == 0 {
                return true;
            }
            let Some(parent) = self.item(item.parent) else {
                return false;
            };
            if parent.state & 0x20 == 0 {
                return false;
            }
            current = parent.id;
        }
        false
    }
    pub(crate) fn first_child(&self, parent: u32) -> u32 {
        self.items
            .iter()
            .find(|item| item.parent == parent)
            .map_or(0, |item| item.id)
    }
    pub(crate) fn has_children(&self, id: u32) -> bool {
        self.first_child(id) != 0 || self.item(id).is_some_and(|item| item.children != 0)
    }
    pub(crate) fn visible_ids(&self) -> Result<Vec<u32>, E> {
        let mut ids = Vec::new();
        ids.try_reserve_exact(self.items.len())
            .map_err(|_| E::Allocation)?;
        ids.extend(
            self.items
                .iter()
                .filter(|item| self.visible(item.id))
                .map(|item| item.id),
        );
        Ok(ids)
    }
    pub(crate) fn state(&self, item: &Item32) -> u32 {
        (item.state & !0xa)
            | if self.selected == item.id { 2 } else { 0 }
            | if self.drop_target == item.id { 8 } else { 0 }
    }
    pub(crate) fn check_text(&self, old: Option<u32>, text: &[u16]) -> Result<(), E> {
        if text.len() > MAX_TEXT {
            return Err(E::Win32(87));
        }
        let total: usize = self
            .items
            .iter()
            .filter(|item| Some(item.id) != old)
            .map(|item| item.text.len())
            .sum();
        if total + text.len() > MAX_TEXT_UNITS {
            return Err(E::Capacity);
        }
        Ok(())
    }
    pub(crate) fn prepare_insert(
        &mut self,
        parent: u32,
        after: u32,
        item: &Item32,
    ) -> Result<usize, E> {
        if self.items.len() == MAX_ITEMS {
            return Err(E::Capacity);
        }
        if parent != 0 && self.item(parent).is_none() {
            return Err(E::Win32(87));
        }
        self.check_text(None, &item.text)?;
        let position = if after == FIRST {
            self.items
                .iter()
                .position(|item| item.parent == parent)
                .unwrap_or_else(|| {
                    if parent == 0 {
                        self.items.len()
                    } else {
                        self.items
                            .iter()
                            .position(|item| item.id == parent)
                            .unwrap()
                            + 1
                    }
                })
        } else if after == LAST || after == 0 {
            if parent == 0 {
                self.items.len()
            } else {
                self.items
                    .iter()
                    .rposition(|item| self.descendant(item.id, parent))
                    .unwrap()
                    + 1
            }
        } else {
            if self.item(after).is_none_or(|item| item.parent != parent) {
                return Err(E::Win32(87));
            }
            self.items
                .iter()
                .rposition(|item| self.descendant(item.id, after))
                .unwrap()
                + 1
        };
        self.items.try_reserve(1).map_err(|_| E::Allocation)?;
        Ok(position)
    }
    pub(crate) fn insert(&mut self, index: usize, item: Item32) {
        self.items.insert(index, item);
    }
    pub(crate) fn remove_leaf(&mut self, id: u32) {
        if let Some(index) = self.items.iter().position(|item| item.id == id) {
            self.items.remove(index);
            if self.selected == id {
                self.selected = 0;
            }
            if self.drop_target == id {
                self.drop_target = 0;
            }
            if self.first == id {
                self.first = 0;
            }
        }
    }
    pub(crate) fn deletion_leaf(&self, root: u32) -> u32 {
        self.items
            .iter()
            .rev()
            .find(|item| self.descendant(item.id, root))
            .map_or(0, |item| item.id)
    }
}
#[cfg(test)]
mod tests;
