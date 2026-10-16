use crate::windows::registry::{
    PreparedRegistry32, Registry32, RegistryChangeKind32 as Kind, RegistryError32 as E,
    RegistryKeyId32 as Id,
    model::{Hive32, Key32, Target32},
    path::{self, MACHINE_CLASSES},
    prepared::Action32,
};
use crate::windows::text::ordinal_key;

impl Registry32 {
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
    pub(in crate::windows::registry) fn prepare_create_internal(
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
}
