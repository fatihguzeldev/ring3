use crate::windows::{
    user32::{Point32, Rect32, User32Error32 as E},
    window::WindowHandle32,
};
pub(crate) const MAX_ITEMS: usize = 4096;
pub(crate) const MAX_SUBITEMS: usize = 4096;
pub(crate) const MAX_TEXT: usize = 65535;
const MAX_TEXT_UNITS: usize = 524288;
const MAX_CELLS: usize = 65536;
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Cell32 {
    pub subitem: usize,
    pub text: Vec<u16>,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Item32 {
    pub key: u64,
    pub revision: u64,
    pub cells: Vec<Cell32>,
    pub state: u32,
    pub image: i32,
    pub data: u32,
    pub indent: i32,
    pub position: Option<Point32>,
}
impl Item32 {
    pub(crate) fn text(&self, subitem: usize) -> &[u16] {
        self.cells
            .iter()
            .find(|cell| cell.subitem == subitem)
            .map_or(&[], |cell| cell.text.as_slice())
    }
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Patch32 {
    pub key: u64,
    pub revision: u64,
    pub subitem: usize,
    pub mask: u32,
    pub state: u32,
    pub state_mask: u32,
    pub image: i32,
    pub data: u32,
    pub indent: i32,
    pub text: Option<Vec<u16>>,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Change32 {
    pub id: u64,
    pub patches: Vec<Patch32>,
    pub cursor: usize,
    pub insertion: Option<(usize, Item32)>,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Column32 {
    pub key: u64,
    pub subitem: usize,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ListView32 {
    pub items: Vec<Item32>,
    pub columns: Vec<Column32>,
    pub header: Option<WindowHandle32>,
    pub font: u32,
    pub unicode: bool,
    pub top: usize,
    pub horizontal: i32,
    pub width: i32,
    pub anchor: Option<u64>,
    pub extended: u32,
    pub background: Option<u32>,
    pub text_color: Option<u32>,
    pub text_background: Option<u32>,
    pub redraw: bool,
    pub changes: Vec<Change32>,
    next_item: u64,
    next_change: u64,
}
impl Default for ListView32 {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            columns: Vec::new(),
            header: None,
            font: 0,
            unicode: true,
            top: 0,
            horizontal: 0,
            width: 120,
            anchor: None,
            extended: 0,
            background: None,
            text_color: None,
            text_background: None,
            redraw: true,
            changes: Vec::new(),
            next_item: 1,
            next_change: 1,
        }
    }
}
impl ListView32 {
    fn units(&self) -> usize {
        self.items
            .iter()
            .flat_map(|item| &item.cells)
            .map(|cell| cell.text.len())
            .sum()
    }
    fn cells(&self) -> usize {
        self.items.iter().map(|item| item.cells.len()).sum()
    }
    fn check_insert(&self, item: &Item32) -> Result<(), E> {
        if self.items.len() == MAX_ITEMS || self.cells() + item.cells.len() > MAX_CELLS {
            return Err(E::Capacity);
        }
        if item
            .cells
            .iter()
            .any(|cell| cell.subitem >= MAX_SUBITEMS || cell.text.len() > MAX_TEXT)
        {
            return Err(E::Win32(87));
        }
        if self.units() + item.cells.iter().map(|cell| cell.text.len()).sum::<usize>()
            > MAX_TEXT_UNITS
        {
            return Err(E::Capacity);
        }
        self.next_item.checked_add(1).ok_or(E::Capacity)?;
        Ok(())
    }
    pub(crate) fn prepare_insert(&mut self, item: &Item32) -> Result<(), E> {
        self.check_insert(item)?;
        self.items.try_reserve(1).map_err(|_| E::Allocation)
    }
    pub(crate) fn insert(&mut self, index: usize, mut item: Item32) -> usize {
        let index = index.min(self.items.len());
        item.key = self.next_item;
        self.next_item += 1;
        item.revision = 1;
        self.items.insert(index, item);
        index
    }
    pub(crate) fn index(&self, key: u64) -> Option<usize> {
        self.items.iter().position(|item| item.key == key)
    }
    pub(crate) fn delete(&mut self, index: usize) -> Item32 {
        let item = self.items.remove(index);
        if self.anchor == Some(item.key) {
            self.anchor = None
        }
        self.top = self.top.min(self.items.len().saturating_sub(1));
        item
    }
    pub(crate) fn prepare_change(
        &mut self,
        patches: Vec<Patch32>,
        limit: usize,
    ) -> Result<Change32, E> {
        if self.changes.len() >= limit {
            return Err(E::Capacity);
        }
        self.next_change.checked_add(1).ok_or(E::Capacity)?;
        let mut units = self.units();
        let mut cells = self.cells();
        for patch in &patches {
            let Some(index) = self.index(patch.key) else {
                return Err(E::InvalidRequest);
            };
            let item = &self.items[index];
            if item.revision != patch.revision || patch.subitem >= MAX_SUBITEMS {
                return Err(E::InvalidRequest);
            }
            item.revision.checked_add(1).ok_or(E::Capacity)?;
            if let Some(text) = &patch.text {
                if text.len() > MAX_TEXT {
                    return Err(E::Win32(87));
                }
                units = units - item.text(patch.subitem).len() + text.len();
                if !item.cells.iter().any(|cell| cell.subitem == patch.subitem) {
                    cells += 1
                }
            }
        }
        if units > MAX_TEXT_UNITS || cells > MAX_CELLS {
            return Err(E::Capacity);
        }
        for patch in &patches {
            if patch.text.is_some() {
                let index = self.index(patch.key).unwrap();
                if !self.items[index]
                    .cells
                    .iter()
                    .any(|cell| cell.subitem == patch.subitem)
                {
                    self.items[index]
                        .cells
                        .try_reserve(1)
                        .map_err(|_| E::Allocation)?
                }
            }
        }
        self.changes.try_reserve(1).map_err(|_| E::Allocation)?;
        Ok(Change32 {
            id: self.next_change,
            patches,
            cursor: 0,
            insertion: None,
        })
    }
    pub(crate) fn publish_change(&mut self, change: Change32) -> u64 {
        let id = change.id;
        debug_assert_eq!(id, self.next_change);
        self.next_change += 1;
        self.changes.push(change);
        id
    }
    pub(crate) fn change(&self, id: u64) -> Option<&Change32> {
        self.changes.iter().find(|change| change.id == id)
    }
    pub(crate) fn advance(&mut self, id: u64) {
        if let Some(change) = self.changes.iter_mut().find(|change| change.id == id) {
            change.cursor += 1
        }
    }
    pub(crate) fn retire(&mut self, id: u64) {
        self.changes.retain(|change| change.id != id)
    }
    pub(crate) fn apply(&mut self, id: u64, single: bool) -> Result<bool, E> {
        let Some(at) = self.changes.iter().position(|change| change.id == id) else {
            return Ok(false);
        };
        let cursor = self.changes[at].cursor;
        let Some(patch) = self.changes[at].patches.get(cursor) else {
            return Ok(false);
        };
        let Some(index) = self.index(patch.key) else {
            return Ok(false);
        };
        if self.items[index].revision != patch.revision {
            return Ok(false);
        }
        if let Some(text) = &patch.text {
            let units = self.units() - self.items[index].text(patch.subitem).len() + text.len();
            let cells = self.cells()
                + usize::from(
                    !self.items[index]
                        .cells
                        .iter()
                        .any(|cell| cell.subitem == patch.subitem),
                );
            if units > MAX_TEXT_UNITS || cells > MAX_CELLS {
                return Err(E::Capacity);
            }
        }
        if patch.mask & 8 != 0 {
            let next =
                (self.items[index].state & !patch.state_mask) | (patch.state & patch.state_mask);
            if self.items.iter().enumerate().any(|(other, item)| {
                other != index
                    && (next & 1 != 0 && item.state & 1 != 0
                        || single && next & 2 != 0 && item.state & 2 != 0)
            }) {
                return Ok(false);
            }
        }
        let patch = &mut self.changes[at].patches[cursor];
        let item = &mut self.items[index];
        if let Some(text) = patch.text.take() {
            if let Some(cell) = item
                .cells
                .iter_mut()
                .find(|cell| cell.subitem == patch.subitem)
            {
                cell.text = text
            } else {
                item.cells.push(Cell32 {
                    subitem: patch.subitem,
                    text,
                })
            }
        }
        if patch.mask & 8 != 0 {
            item.state = (item.state & !patch.state_mask) | (patch.state & patch.state_mask)
        }
        if patch.mask & 2 != 0 {
            item.image = patch.image
        }
        if patch.mask & 4 != 0 {
            item.data = patch.data
        }
        if patch.mask & 16 != 0 {
            item.indent = patch.indent
        }
        item.revision += 1;
        patch.revision = item.revision;
        Ok(true)
    }
    pub(crate) fn finish_insertion(
        &mut self,
        id: u64,
        single: bool,
    ) -> Result<Option<(usize, u32, u32)>, E> {
        let Some(at) = self.changes.iter().position(|change| change.id == id) else {
            return Ok(None);
        };
        let Some((_, item)) = self.changes[at].insertion.as_ref() else {
            return Ok(None);
        };
        self.check_insert(item)?;
        self.items.try_reserve(1).map_err(|_| E::Allocation)?;
        let (index, mut item) = self.changes[at]
            .insertion
            .take()
            .expect("same retained insertion");
        if self.items.iter().any(|other| other.state & 1 != 0) {
            item.state &= !1;
        }
        if single && self.items.iter().any(|other| other.state & 2 != 0) {
            item.state &= !2;
        }
        let state = item.state;
        let data = item.data;
        let index = self.insert(index, item);
        Ok(Some((index, state, data)))
    }
    pub(crate) fn focused(&self) -> Option<usize> {
        self.items.iter().position(|item| item.state & 1 != 0)
    }
    pub(crate) fn rect(
        &self,
        index: usize,
        view: u32,
        width: i32,
        height: i32,
        row_height: i32,
        header: i32,
    ) -> Rect32 {
        let row_height = row_height.max(1);
        let available = (height - header).max(1);
        let item_width = self.width.max(1);
        let (x, y, w, h) = match view & 3 {
            1 => (
                -self.horizontal,
                header.saturating_add((index as i32 - self.top as i32).saturating_mul(row_height)),
                width.max(item_width),
                row_height,
            ),
            3 => {
                let rows = (available / row_height).max(1);
                let n = index as i32 - self.top as i32;
                (
                    n.div_euclid(rows)
                        .saturating_mul(item_width)
                        .saturating_sub(self.horizontal),
                    header.saturating_add(n.rem_euclid(rows).saturating_mul(row_height)),
                    item_width,
                    row_height,
                )
            }
            _ => {
                let (w, h) = if view & 3 == 0 {
                    (80, row_height.max(64))
                } else {
                    (item_width, row_height)
                };
                let columns = (width / w).max(1);
                let point = self
                    .items
                    .get(index)
                    .and_then(|item| item.position)
                    .unwrap_or(Point32 {
                        x: (index as i32 % columns).saturating_mul(w),
                        y: (index as i32 / columns).saturating_mul(h),
                    });
                (
                    point.x - self.horizontal,
                    point
                        .y
                        .saturating_sub((self.top as i32).saturating_mul(row_height)),
                    w,
                    h,
                )
            }
        };
        Rect32 {
            left: x,
            top: y,
            right: x.saturating_add(w),
            bottom: y.saturating_add(h),
        }
    }
}
#[cfg(test)]
mod tests;
