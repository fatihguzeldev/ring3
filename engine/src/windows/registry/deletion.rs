use super::{
    PreparedRegistry32, Registry32, RegistryChangeKind32 as Kind, RegistryError32 as E,
    model::Value32, path, prepared::Action32,
};
use crate::windows::text::ordinal_key;
impl Registry32 {
    pub fn prepare_delete_value(
        &mut self,
        handle: u32,
        name: &[u16],
        modified: u64,
    ) -> Result<PreparedRegistry32, E> {
        if name.len() > 16383 || name.contains(&0) {
            return Err(E::InvalidParameter);
        }
        let (target, _) = self.target(handle, 2)?;
        self.prepare_delete_value_at(target.primary, name, modified)
    }
    pub(super) fn prepare_delete_value_at(
        &mut self,
        id: super::RegistryKeyId32,
        name: &[u16],
        modified: u64,
    ) -> Result<PreparedRegistry32, E> {
        if name.len() > 16383 || name.contains(&0) {
            return Err(E::InvalidParameter);
        }
        let key = self.live_key(id)?;
        let ordinal = ordinal_key(name);
        let at = key
            .values
            .binary_search_by(|value| value.ordinal.cmp(&ordinal))
            .map_err(|_| E::FileNotFound)?;
        let mut prepared = self.preparation()?;
        prepared.bytes -= Value32::bytes(&key.values[at]);
        prepared.values -= 1;
        if !key.volatile {
            self.stage_change(
                &mut prepared,
                Kind::DeleteValue,
                key.hive,
                key.view,
                &key.path,
                name,
                None,
                modified,
            )?;
        }
        self.changes
            .try_reserve(prepared.changes.len())
            .map_err(|_| E::NotEnoughMemory)?;
        prepared
            .actions
            .try_reserve(2)
            .map_err(|_| E::NotEnoughMemory)?;
        prepared.actions.push(Action32::DeleteValue(id, at));
        prepared.actions.push(Action32::Modified(id, modified));
        Ok(prepared)
    }
    pub fn prepare_delete_key(
        &mut self,
        handle: u32,
        subkey: &[u16],
        view_flags: u32,
        recursive: bool,
        clear_only: bool,
        modified: u64,
    ) -> Result<PreparedRegistry32, E> {
        if recursive {
            self.target(handle, 0x10009)?;
        }
        let (target, _) = self.resolved(handle, subkey, view_flags)?;
        let key = self.live_key(target.primary)?;
        if recursive && clear_only && !key.values.is_empty() {
            self.target(handle, 2)?;
        }
        if key.path.is_empty() {
            return Err(E::AccessDenied);
        }
        let mut ids = Vec::new();
        for &id in &self.namespace {
            let candidate = self.key(id)?;
            if candidate.hive == key.hive
                && candidate.view == key.view
                && path::starts_with(&candidate.ordinal, &key.ordinal)
                && (id != target.primary || !clear_only)
            {
                ids.try_reserve(1).map_err(|_| E::NotEnoughMemory)?;
                ids.push(id);
            }
        }
        if !recursive && ids.len() > 1 {
            return Err(E::AccessDenied);
        }
        let mut prepared = self.preparation()?;
        if !key.volatile {
            self.stage_change(
                &mut prepared,
                Kind::DeleteTree,
                key.hive,
                key.view,
                &key.path,
                &[],
                None,
                modified,
            )?;
            if clear_only {
                self.stage_change(
                    &mut prepared,
                    Kind::CreateKey,
                    key.hive,
                    key.view,
                    &key.path,
                    &[],
                    Some(target.primary),
                    modified,
                )?;
            }
        }
        for &id in &ids {
            let removed = self.key(id)?;
            prepared.values -= removed.values.len();
            prepared.bytes -= if removed.handles == 0 {
                removed.bytes()
            } else {
                removed.values.iter().map(Value32::bytes).sum()
            };
        }
        prepared
            .actions
            .try_reserve(ids.len() + key.values.len() + 2)
            .map_err(|_| E::NotEnoughMemory)?;
        if clear_only {
            prepared.values -= key.values.len();
            prepared.bytes -= key.values.iter().map(Value32::bytes).sum::<usize>();
            for at in (0..key.values.len()).rev() {
                prepared
                    .actions
                    .push(Action32::DeleteValue(target.primary, at));
            }
            prepared
                .actions
                .push(Action32::Modified(target.primary, modified));
        }
        prepared.actions.push(Action32::DeleteKeys(ids));
        if prepared.bytes > self.config.limits.bytes as usize {
            return Err(E::NotEnoughMemory);
        }
        self.changes
            .try_reserve(prepared.changes.len())
            .map_err(|_| E::NotEnoughMemory)?;
        Ok(prepared)
    }
}
