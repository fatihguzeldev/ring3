use super::{MenuHandle32, MenuItem32, MenuLimits32, template::Template32, types::Menu32};
use crate::windows::user32::User32Error32 as E;

pub(crate) struct MenuStore32 {
    limits: MenuLimits32,
    menus: Vec<Menu32>,
    item_count: usize,
    text_units: usize,
    next_handle: u32,
    revision: u64,
}

impl MenuStore32 {
    pub(crate) fn new(limits: MenuLimits32) -> Result<Self, E> {
        if !(1..=256).contains(&limits.menus)
            || !(1..=4096).contains(&limits.items)
            || !(1..=512 * 1024).contains(&limits.text_units)
            || !(1..=32).contains(&limits.depth)
        {
            return Err(E::InvalidLimits);
        }
        let mut menus = Vec::new();
        menus
            .try_reserve_exact(limits.menus)
            .map_err(|_| E::Allocation)?;
        Ok(Self {
            limits,
            menus,
            item_count: 0,
            text_units: 0,
            next_handle: 0x4d00_0000,
            revision: 0,
        })
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn contains(&self, handle: MenuHandle32) -> bool {
        self.index(handle).is_ok()
    }

    pub(crate) fn create(&mut self, popup: bool) -> Result<MenuHandle32, E> {
        self.check_capacity(1, 0, 0)?;
        let handle = self.handle_after(1)?;
        let revision = self.next_revision()?;
        self.menus.push(Menu32 {
            handle,
            popup,
            help: 0,
            items: Vec::new(),
        });
        self.next_handle = handle.0;
        self.revision = revision;
        Ok(handle)
    }

    pub(crate) fn load(&mut self, template: Template32) -> Result<MenuHandle32, E> {
        for (index, item) in template.items.iter().enumerate() {
            let mut parent = item.parent;
            let mut child = index;
            let mut depth = if item.popup { 2 } else { 1 };
            while let Some(index) = parent {
                let parent_item = template
                    .items
                    .get(index)
                    .filter(|item| item.popup)
                    .filter(|_| index < child)
                    .ok_or(E::InvalidRequest)?;
                depth += 1;
                if depth > self.limits.depth {
                    return Err(E::Capacity);
                }
                parent = parent_item.parent;
                child = index;
            }
            if depth > self.limits.depth {
                return Err(E::Capacity);
            }
        }
        let count = template.items.iter().filter(|item| item.popup).count() + 1;
        let units = template.items.iter().try_fold(0usize, |total, item| {
            total.checked_add(item.text.len()).ok_or(E::Capacity)
        })?;
        self.check_capacity(count, template.items.len(), units)?;
        let last = self.handle_after(count)?;
        let revision = self.next_revision()?;
        let mut staged = Vec::new();
        staged.try_reserve_exact(count).map_err(|_| E::Allocation)?;
        let mut submenus = Vec::new();
        submenus
            .try_reserve_exact(template.items.len())
            .map_err(|_| E::Allocation)?;
        submenus.resize(template.items.len(), None);
        staged.push(Menu32 {
            handle: self.handle_after(1)?,
            popup: false,
            help: 0,
            items: Vec::new(),
        });
        for (index, item) in template.items.iter().enumerate() {
            if item.popup {
                let slot = staged.len();
                let handle = self.handle_after(slot + 1)?;
                submenus[index] = Some(slot);
                staged.push(Menu32 {
                    handle,
                    popup: true,
                    help: item.help,
                    items: Vec::new(),
                });
            }
        }
        let item_count = template.items.len();
        for (index, item) in template.items.into_iter().enumerate() {
            let parent = item.parent.map_or(Ok(0), |parent| {
                if parent >= index {
                    return Err(E::InvalidRequest);
                }
                submenus
                    .get(parent)
                    .copied()
                    .flatten()
                    .ok_or(E::InvalidRequest)
            })?;
            let submenu = submenus[index].map(|slot| staged[slot].handle);
            let id = if template.extended {
                item.id
            } else {
                submenu.map_or(item.id, MenuHandle32::get)
            };
            staged[parent]
                .items
                .try_reserve(1)
                .map_err(|_| E::Allocation)?;
            staged[parent].items.push(MenuItem32 {
                kind: item.kind,
                state: item.state,
                submenu,
                ..MenuItem32::command(id, item.text)
            });
        }
        let root = staged[0].handle;
        self.menus.extend(staged);
        self.next_handle = last.0;
        self.item_count += item_count;
        self.text_units += units;
        self.revision = revision;
        Ok(root)
    }

    pub(crate) fn count(&self, handle: MenuHandle32) -> Result<usize, E> {
        Ok(self.menus[self.index(handle)?].items.len())
    }

    pub(crate) fn popup(&self, handle: MenuHandle32) -> Result<bool, E> {
        Ok(self.menus[self.index(handle)?].popup)
    }

    pub(crate) fn help(&self, handle: MenuHandle32) -> Result<u32, E> {
        Ok(self.menus[self.index(handle)?].help)
    }

    pub(crate) fn set_help(&mut self, handle: MenuHandle32, help: u32) -> Result<(), E> {
        let index = self.index(handle)?;
        let revision = self.next_revision()?;
        self.menus[index].help = help;
        self.revision = revision;
        Ok(())
    }

    pub(crate) fn item(
        &self,
        handle: MenuHandle32,
        key: u32,
        position: bool,
    ) -> Result<&MenuItem32, E> {
        let (menu, item) = self.find(handle, key, position)?;
        Ok(&self.menus[menu].items[item])
    }

    pub(crate) fn insert(
        &mut self,
        handle: MenuHandle32,
        position: usize,
        item: MenuItem32,
    ) -> Result<(), E> {
        let index = self.index(handle)?;
        if position > self.menus[index].items.len() {
            return Err(E::Win32(1456));
        }
        self.check_capacity(0, 1, item.text.len())?;
        self.check_submenu(handle, item.submenu)?;
        let revision = self.next_revision()?;
        self.menus[index]
            .items
            .try_reserve(1)
            .map_err(|_| E::Allocation)?;
        self.text_units += item.text.len();
        self.item_count += 1;
        self.menus[index].items.insert(position, item);
        self.revision = revision;
        Ok(())
    }

    pub(crate) fn replace(
        &mut self,
        handle: MenuHandle32,
        key: u32,
        position: bool,
        item: MenuItem32,
    ) -> Result<(), E> {
        let (menu, offset) = self.find(handle, key, position)?;
        let old_units = self.menus[menu].items[offset].text.len();
        if item.text.len() > self.limits.text_units - (self.text_units - old_units) {
            return Err(E::Capacity);
        }
        self.check_submenu(self.menus[menu].handle, item.submenu)?;
        let revision = self.next_revision()?;
        self.text_units = self.text_units - old_units + item.text.len();
        self.menus[menu].items[offset] = item;
        self.revision = revision;
        Ok(())
    }

    pub(crate) fn set_state(
        &mut self,
        handle: MenuHandle32,
        key: u32,
        position: bool,
        mask: u32,
        value: u32,
    ) -> Result<u32, E> {
        let (menu, offset) = self.find(handle, key, position)?;
        let revision = self.next_revision()?;
        let item = &mut self.menus[menu].items[offset];
        let previous = item.state;
        item.state = (previous & !mask) | (value & mask);
        self.revision = revision;
        Ok(previous)
    }

    pub(crate) fn remove(
        &mut self,
        handle: MenuHandle32,
        key: u32,
        position: bool,
        destroy_submenu: bool,
    ) -> Result<(), E> {
        let (menu, offset) = self.find(handle, key, position)?;
        let revision = self.next_revision()?;
        let destroyed = match (destroy_submenu, self.menus[menu].items[offset].submenu) {
            (true, Some(submenu)) => self.subtree(submenu)?,
            _ => Vec::new(),
        };
        let removed = self.menus[menu].items.remove(offset);
        self.item_count -= 1;
        self.text_units -= removed.text.len();
        self.destroy_collected(&destroyed);
        self.revision = revision;
        Ok(())
    }

    pub(crate) fn destroy(&mut self, handle: MenuHandle32) -> Result<(), E> {
        let destroyed = self.subtree(handle)?;
        let revision = self.next_revision()?;
        self.destroy_collected(&destroyed);
        self.revision = revision;
        Ok(())
    }

    fn index(&self, handle: MenuHandle32) -> Result<usize, E> {
        self.menus
            .iter()
            .position(|menu| menu.handle == handle)
            .ok_or(E::Win32(1401))
    }

    fn handle_after(&self, additional: usize) -> Result<MenuHandle32, E> {
        self.next_handle
            .checked_add(additional as u32)
            .filter(|&handle| handle < 0x4e00_0000)
            .map(MenuHandle32)
            .ok_or(E::Capacity)
    }

    fn next_revision(&self) -> Result<u64, E> {
        self.revision.checked_add(1).ok_or(E::Capacity)
    }

    fn check_capacity(&self, menus: usize, items: usize, units: usize) -> Result<(), E> {
        if menus > self.limits.menus - self.menus.len()
            || items > self.limits.items - self.item_count
            || units > self.limits.text_units - self.text_units
        {
            return Err(E::Capacity);
        }
        Ok(())
    }

    fn find(&self, handle: MenuHandle32, key: u32, position: bool) -> Result<(usize, usize), E> {
        let root = self.index(handle)?;
        if position {
            return ((key as usize) < self.menus[root].items.len())
                .then_some((root, key as usize))
                .ok_or(E::Win32(1456));
        }
        let mut visited = [false; 256];
        self.find_command(root, key, &mut visited)
            .ok_or(E::Win32(1456))
    }

    fn find_command(
        &self,
        index: usize,
        key: u32,
        visited: &mut [bool; 256],
    ) -> Option<(usize, usize)> {
        if visited[index] {
            return None;
        }
        visited[index] = true;
        for (offset, item) in self.menus[index].items.iter().enumerate() {
            if item.id == key {
                return Some((index, offset));
            }
            if let Some(child) = item.submenu.and_then(|handle| self.index(handle).ok())
                && let Some(found) = self.find_command(child, key, visited)
            {
                return Some(found);
            }
        }
        None
    }

    fn check_submenu(&self, parent: MenuHandle32, child: Option<MenuHandle32>) -> Result<(), E> {
        let Some(child) = child else {
            return Ok(());
        };
        self.index(child)?;
        let mut visited = [false; 256];
        if self.reaches(child, parent, &mut visited) {
            return Err(E::Win32(87));
        }
        let mut descendants = [false; 256];
        let down = self.depth(child, &mut descendants, &mut [0; 256])?;
        let mut visited = [false; 256];
        let mut paths = [0; 256];
        for menu in &self.menus {
            if self
                .path_depth(menu.handle, parent, &mut visited, &mut paths)?
                .is_some_and(|up| up + down > self.limits.depth)
            {
                return Err(E::Capacity);
            }
        }
        Ok(())
    }

    fn reaches(&self, from: MenuHandle32, to: MenuHandle32, visited: &mut [bool; 256]) -> bool {
        if from == to {
            return true;
        }
        let Ok(index) = self.index(from) else {
            return false;
        };
        if visited[index] {
            return false;
        }
        visited[index] = true;
        self.menus[index]
            .items
            .iter()
            .filter_map(|item| item.submenu)
            .any(|child| self.reaches(child, to, visited))
    }

    fn depth(
        &self,
        from: MenuHandle32,
        visited: &mut [bool; 256],
        memo: &mut [usize; 256],
    ) -> Result<usize, E> {
        let index = self.index(from)?;
        if memo[index] != 0 {
            return Ok(memo[index]);
        }
        if visited[index] {
            return Err(E::InvalidRequest);
        }
        visited[index] = true;
        let mut depth = 1;
        for child in self.menus[index]
            .items
            .iter()
            .filter_map(|item| item.submenu)
            .filter(|&child| self.contains(child))
        {
            depth = depth.max(1 + self.depth(child, visited, memo)?);
        }
        visited[index] = false;
        memo[index] = depth;
        Ok(depth)
    }

    fn path_depth(
        &self,
        from: MenuHandle32,
        to: MenuHandle32,
        visited: &mut [bool; 256],
        memo: &mut [usize; 256],
    ) -> Result<Option<usize>, E> {
        if from == to {
            return Ok(Some(1));
        }
        let index = self.index(from)?;
        if memo[index] != 0 {
            return Ok((memo[index] != usize::MAX).then_some(memo[index]));
        }
        if visited[index] {
            return Err(E::InvalidRequest);
        }
        visited[index] = true;
        let mut depth: Option<usize> = None;
        for child in self.menus[index]
            .items
            .iter()
            .filter_map(|item| item.submenu)
            .filter(|&child| self.contains(child))
        {
            if let Some(child_depth) = self.path_depth(child, to, visited, memo)? {
                depth =
                    Some(depth.map_or(child_depth + 1, |previous| previous.max(child_depth + 1)));
            }
        }
        visited[index] = false;
        memo[index] = depth.unwrap_or(usize::MAX);
        Ok(depth)
    }

    fn subtree(&self, root: MenuHandle32) -> Result<Vec<MenuHandle32>, E> {
        self.index(root)?;
        let mut handles = Vec::new();
        handles
            .try_reserve_exact(self.menus.len())
            .map_err(|_| E::Allocation)?;
        handles.push(root);
        let mut at = 0;
        while at < handles.len() {
            let index = self.index(handles[at])?;
            for child in self.menus[index]
                .items
                .iter()
                .filter_map(|item| item.submenu)
            {
                if self.contains(child) && !handles.contains(&child) {
                    handles.push(child);
                }
            }
            at += 1;
        }
        Ok(handles)
    }

    fn destroy_collected(&mut self, handles: &[MenuHandle32]) {
        self.menus.retain(|menu| {
            if handles.contains(&menu.handle) {
                self.item_count -= menu.items.len();
                self.text_units -= menu.items.iter().map(|item| item.text.len()).sum::<usize>();
                false
            } else {
                true
            }
        });
    }
}
