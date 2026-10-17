use super::{E, Menu32, MenuItem32, MenuHandle32, MenuStore32, Template32};

pub(crate) struct PreparedMenuLoad32 {
    revision: u64,
    staged: Vec<Menu32>,
    root: MenuHandle32,
    last: MenuHandle32,
    item_count: usize,
    text_units: usize,
    next_revision: u64,
}

impl PreparedMenuLoad32 {
    pub(crate) fn root(&self) -> MenuHandle32 {
        self.root
    }
}

impl MenuStore32 {
    pub(crate) fn prepare_load(&self, template: Template32) -> Result<PreparedMenuLoad32, E> {
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
        Ok(PreparedMenuLoad32 {
            revision: self.revision,
            staged,
            root,
            last,
            item_count,
            text_units: units,
            next_revision: revision,
        })
    }

    pub(crate) fn check_load(&self, prepared: &PreparedMenuLoad32) -> Result<(), E> {
        if prepared.revision != self.revision {
            return Err(E::InvalidRequest);
        }
        Ok(())
    }

    pub(crate) fn commit_load(&mut self, prepared: PreparedMenuLoad32) -> MenuHandle32 {
        self.check_load(&prepared)
            .expect("prepared menu load is current");
        self.menus.extend(prepared.staged);
        self.next_handle = prepared.last.0;
        self.item_count += prepared.item_count;
        self.text_units += prepared.text_units;
        self.revision = prepared.next_revision;
        prepared.root
    }

    pub(crate) fn load(&mut self, template: Template32) -> Result<MenuHandle32, E> {
        let prepared = self.prepare_load(template)?;
        Ok(self.commit_load(prepared))
    }
}

#[cfg(test)]
mod tests;
