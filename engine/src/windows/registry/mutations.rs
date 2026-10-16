use super::{
    PreparedRegistry32, Registry32, RegistryChangeId32, RegistryChangeKind32 as Kind,
    RegistryError32 as E, RegistryKeyId32 as Id, RegistryView32 as View,
    model::{Change32, Handle32, Hive32, Key32, Target32, Value32},
    path::{self, MACHINE_CLASSES},
    prepared::Action32,
};
use crate::windows::text::ordinal_key;
impl Registry32 {
    pub(super) fn prepared_handle(
        &mut self,
        prepared: &mut PreparedRegistry32,
        target: Target32,
        view: View,
        rights: u32,
    ) -> Result<(), E> {
        let slot = self
            .handles
            .iter()
            .position(Option::is_none)
            .ok_or(E::NotEnoughMemory)?;
        let value = prepared
            .next_handle
            .checked_add(4)
            .ok_or(E::NotEnoughMemory)?;
        for id in std::iter::once(target.primary).chain(target.fallback) {
            if let Ok(key) = self.key(id) {
                key.handles.checked_add(1).ok_or(E::NotEnoughMemory)?;
            }
        }
        self.handle_index
            .try_reserve(1)
            .map_err(|_| E::NotEnoughMemory)?;
        prepared.handle = Some((
            slot,
            Handle32 {
                value,
                target,
                access: rights,
                view,
            },
        ));
        prepared.next_handle = value;
        prepared.result = value;
        Ok(())
    }
    pub fn prepare_open(
        &mut self,
        handle: u32,
        subkey: &[u16],
        desired: u32,
    ) -> Result<PreparedRegistry32, E> {
        let rights = path::access(desired)?;
        let (target, view) = self.resolved(handle, subkey, desired)?;
        let mut prepared = self.preparation()?;
        if subkey.is_empty() && handle & 0x8000_0000 != 0 && desired & 0x300 == 0 {
            prepared.result = handle;
            return Ok(prepared);
        }
        self.prepared_handle(&mut prepared, target, view, rights)?;
        prepared.disposition = 2;
        Ok(prepared)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_create(
        &mut self,
        handle: u32,
        subkey: &[u16],
        desired: u32,
        class: &[u16],
        volatile: bool,
        modified: u64,
        dirty: bool,
    ) -> Result<PreparedRegistry32, E> {
        self.prepare_create_internal(
            handle, subkey, desired, class, volatile, modified, dirty, true,
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepare_create_internal(
        &mut self,
        handle: u32,
        subkey: &[u16],
        desired: u32,
        class: &[u16],
        volatile: bool,
        modified: u64,
        dirty: bool,
        publish_handle: bool,
    ) -> Result<PreparedRegistry32, E> {
        let desired_rights = path::access(desired)?;
        let subkey = path::path(subkey)?;
        if class.len() > 255 || class.contains(&0) {
            return Err(E::InvalidParameter);
        }
        match self.resolved(handle, &subkey, desired) {
            Ok((target, view)) => {
                let mut prepared = self.preparation()?;
                if publish_handle {
                    self.prepared_handle(&mut prepared, target, view, desired_rights)?;
                }
                prepared.disposition = 2;
                return Ok(prepared);
            }
            Err(E::FileNotFound) => {}
            Err(error) => return Err(error),
        }
        let (parent, inherited) = self.target(handle, 0)?;
        let view = path::view(self.config.separate_views, desired, inherited)?;
        let base = self.live_key(parent.primary)?;
        if base.volatile && !volatile {
            return Err(E::ChildMustBeVolatile);
        }
        let (hive, full) = if parent.classes {
            let relative = path::joined(&self.relative_classes(parent)?, &subkey)?;
            (
                Hive32::Machine,
                path::joined(
                    &MACHINE_CLASSES.encode_utf16().collect::<Vec<_>>(),
                    &relative,
                )?,
            )
        } else {
            (base.hive, path::joined(&base.path, &subkey)?)
        };
        let inherited_volatile = base.volatile;
        let mut prepared = self.preparation()?;
        let mut used = Vec::new();
        let mut target = None;
        for end in full
            .iter()
            .enumerate()
            .filter_map(|(index, &unit)| (unit == 92).then_some(index))
            .chain(std::iter::once(full.len()))
        {
            let prefix = &full[..end];
            if let Some(id) = self.lookup(hive, view, prefix) {
                target = Some(id);
                continue;
            }
            let slot = self
                .keys
                .iter()
                .enumerate()
                .find(|(slot, key)| key.is_none() && !used.contains(slot))
                .map(|(slot, _)| slot)
                .ok_or(E::NotEnoughMemory)?;
            prepared.next_key = prepared.next_key.checked_add(1).ok_or(E::NotEnoughMemory)?;
            let id = Id {
                slot: slot as u32,
                generation: prepared.next_key,
            };
            let key = Key32 {
                generation: id.generation,
                hive,
                view,
                path: prefix.to_vec(),
                ordinal: ordinal_key(prefix),
                class: if end == full.len() {
                    class.to_vec()
                } else {
                    Vec::new()
                },
                modified,
                values: Vec::new(),
                deleted: false,
                handles: 0,
                volatile: volatile || inherited_volatile,
            };
            prepared.bytes = prepared
                .bytes
                .checked_add(key.bytes())
                .ok_or(E::NotEnoughMemory)?;
            if dirty && !key.volatile {
                self.stage_change(
                    &mut prepared,
                    Kind::CreateKey,
                    hive,
                    view,
                    prefix,
                    &[],
                    Some(id),
                    modified,
                )?;
            }
            prepared
                .actions
                .try_reserve(1)
                .map_err(|_| E::NotEnoughMemory)?;
            prepared.actions.push(Action32::Create(slot, key));
            used.try_reserve(1).map_err(|_| E::NotEnoughMemory)?;
            used.push(slot);
            target = Some(id);
        }
        if prepared.bytes > self.config.limits.bytes as usize {
            return Err(E::NotEnoughMemory);
        }
        self.namespace
            .try_reserve(used.len())
            .map_err(|_| E::NotEnoughMemory)?;
        self.changes
            .try_reserve(prepared.changes.len())
            .map_err(|_| E::NotEnoughMemory)?;
        if publish_handle {
            self.prepared_handle(
                &mut prepared,
                Target32 {
                    primary: target.ok_or(E::InvalidParameter)?,
                    fallback: None,
                    classes: parent.classes,
                },
                view,
                desired_rights,
            )?;
        }
        prepared.disposition = 1;
        Ok(prepared)
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn stage_change(
        &self,
        prepared: &mut PreparedRegistry32,
        kind: Kind,
        hive: Hive32,
        view: View,
        path: &[u16],
        name: &[u16],
        key: Option<Id>,
        modified: u64,
    ) -> Result<(), E> {
        let ordinal_path = ordinal_key(path);
        let ordinal_name = ordinal_key(name);
        for change in &self.changes {
            let same = change.hive == hive
                && change.view == view
                && (if kind == Kind::DeleteTree {
                    path::starts_with(&ordinal_key(&change.path), &ordinal_path)
                } else {
                    change.kind == kind
                        && ordinal_key(&change.path) == ordinal_path
                        && ordinal_key(&change.name) == ordinal_name
                        || matches!(kind, Kind::SetValue | Kind::DeleteValue)
                            && matches!(change.kind, Kind::SetValue | Kind::DeleteValue)
                            && ordinal_key(&change.path) == ordinal_path
                            && ordinal_key(&change.name) == ordinal_name
                });
            if same && !prepared.remove_changes.contains(&change.id.sequence) {
                prepared
                    .remove_changes
                    .try_reserve(1)
                    .map_err(|_| E::NotEnoughMemory)?;
                prepared.remove_changes.push(change.id.sequence);
                prepared.bytes -= change.bytes();
            }
        }
        if self.changes.len() - prepared.remove_changes.len() + prepared.changes.len()
            >= self.config.limits.changes as usize
        {
            return Err(E::NotEnoughMemory);
        }
        prepared.next_change = prepared
            .next_change
            .checked_add(1)
            .ok_or(E::NotEnoughMemory)?;
        let change = Change32 {
            id: RegistryChangeId32 {
                epoch: self.epoch,
                sequence: prepared.next_change,
                revision: self.revision + 1,
            },
            kind,
            hive,
            view,
            path: path.to_vec(),
            name: name.to_vec(),
            key,
            modified,
        };
        prepared.bytes = prepared
            .bytes
            .checked_add(change.bytes())
            .ok_or(E::NotEnoughMemory)?;
        prepared
            .changes
            .try_reserve(1)
            .map_err(|_| E::NotEnoughMemory)?;
        prepared.changes.push(change);
        Ok(())
    }
    pub fn prepare_set_value(
        &mut self,
        handle: u32,
        name: &[u16],
        kind: u32,
        bytes: &[u8],
        modified: u64,
        dirty: bool,
    ) -> Result<PreparedRegistry32, E> {
        if name.len() > 16383
            || name.contains(&0)
            || bytes.len() > self.config.limits.bytes as usize
        {
            return Err(E::InvalidParameter);
        }
        let (target, _) = self.target(handle, 2)?;
        self.prepare_set_at(target.primary, name, kind, bytes, modified, dirty)
    }
    pub(super) fn prepare_set_at(
        &mut self,
        id: Id,
        name: &[u16],
        kind: u32,
        bytes: &[u8],
        modified: u64,
        dirty: bool,
    ) -> Result<PreparedRegistry32, E> {
        let key = self.live_key(id)?;
        let ordinal = ordinal_key(name);
        let at = key
            .values
            .binary_search_by(|value| value.ordinal.cmp(&ordinal));
        let mut prepared = self.preparation()?;
        let old = at.ok().map(|at| &key.values[at]);
        let old_bytes = old.map_or(0, Value32::bytes);
        if old.is_none() {
            prepared.values = prepared
                .values
                .checked_add(1)
                .filter(|&values| values <= self.config.limits.values as usize)
                .ok_or(E::NotEnoughMemory)?;
        }
        let mut data = Vec::new();
        data.try_reserve_exact(bytes.len())
            .map_err(|_| E::NotEnoughMemory)?;
        data.extend_from_slice(bytes);
        let value = Value32 {
            name: name.to_vec(),
            ordinal,
            kind,
            bytes: data,
        };
        prepared.bytes = prepared
            .bytes
            .checked_sub(old_bytes)
            .and_then(|value_bytes| value_bytes.checked_add(value.bytes()))
            .ok_or(E::NotEnoughMemory)?;
        if dirty && !key.volatile {
            self.stage_change(
                &mut prepared,
                Kind::SetValue,
                key.hive,
                key.view,
                &key.path,
                name,
                Some(id),
                modified,
            )?;
        }
        if prepared.bytes > self.config.limits.bytes as usize {
            return Err(E::NotEnoughMemory);
        }
        self.keys[id.slot as usize]
            .as_mut()
            .unwrap()
            .values
            .try_reserve(usize::from(at.is_err()))
            .map_err(|_| E::NotEnoughMemory)?;
        self.changes
            .try_reserve(prepared.changes.len())
            .map_err(|_| E::NotEnoughMemory)?;
        prepared
            .actions
            .try_reserve(2)
            .map_err(|_| E::NotEnoughMemory)?;
        prepared
            .actions
            .push(Action32::SetValue(id, at.unwrap_or_else(|at| at), value));
        prepared.actions.push(Action32::Modified(id, modified));
        Ok(prepared)
    }
    pub fn close(&mut self, handle: u32) -> Result<(), E> {
        if handle & 0x8000_0000 != 0 {
            self.root(handle, View::View32)?;
            return Ok(());
        }
        let slot = self.handle_slot(handle)?;
        self.revision.checked_add(1).ok_or(E::NotEnoughMemory)?;
        let handle = self.handles[slot].take().unwrap();
        for id in std::iter::once(handle.target.primary).chain(handle.target.fallback) {
            let key = self.keys[id.slot as usize].as_mut().unwrap();
            key.handles -= 1;
            if key.deleted && key.handles == 0 {
                self.bytes -= key.bytes();
                self.keys[id.slot as usize] = None;
            }
        }
        self.handle_index.retain(|(_, index)| *index != slot);
        self.revision += 1;
        Ok(())
    }
}
