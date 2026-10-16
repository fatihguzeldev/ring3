use crate::windows::registry::{
    PreparedRegistry32, Registry32, RegistryChangeKind32 as Kind, RegistryError32 as E,
    RegistryKeyId32 as Id, model::Value32, prepared::Action32,
};
use crate::windows::text::ordinal_key;

impl Registry32 {
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
    pub(in crate::windows::registry) fn prepare_set_at(
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
}
