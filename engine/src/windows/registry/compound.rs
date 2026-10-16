use super::{
    PreparedRegistry32, Registry32, RegistryChangeKind32, RegistryError32 as E, model::Value32,
    prepared::Action32,
};
use crate::windows::text::ordinal_key;
impl Registry32 {
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_set_path(
        &mut self,
        handle: u32,
        path: &[u16],
        name: &[u16],
        kind: u32,
        bytes: &[u8],
        modified: u64,
        create: bool,
    ) -> Result<PreparedRegistry32, E> {
        self.target(handle, 2)?;
        if name.len() > 16383
            || name.contains(&0)
            || bytes.len() > self.config.limits.bytes as usize
        {
            return Err(E::InvalidParameter);
        }
        match self.resolved(handle, path, 0) {
            Ok((target, _)) => {
                self.prepare_set_at(target.primary, name, kind, bytes, modified, true)
            }
            Err(E::FileNotFound) if create => {
                let mut prepared = self.prepare_create_internal(
                    handle,
                    path,
                    2,
                    &[],
                    false,
                    modified,
                    true,
                    false,
                )?;
                let (slot, key) = prepared
                    .actions
                    .iter_mut()
                    .rev()
                    .find_map(|action| match action {
                        Action32::Create(slot, key) => Some((*slot, key)),
                        _ => None,
                    })
                    .ok_or(E::InvalidData)?;
                let mut data = Vec::new();
                data.try_reserve_exact(bytes.len())
                    .map_err(|_| E::NotEnoughMemory)?;
                data.extend_from_slice(bytes);
                let value = Value32 {
                    name: name.to_vec(),
                    ordinal: ordinal_key(name),
                    kind,
                    bytes: data,
                };
                prepared.bytes = prepared
                    .bytes
                    .checked_add(value.bytes())
                    .ok_or(E::NotEnoughMemory)?;
                prepared.values = prepared
                    .values
                    .checked_add(1)
                    .filter(|&values| values <= self.config.limits.values as usize)
                    .ok_or(E::NotEnoughMemory)?;
                key.values
                    .try_reserve_exact(1)
                    .map_err(|_| E::NotEnoughMemory)?;
                key.values.push(value);
                let (hive, view, path, id, volatile) = (
                    key.hive,
                    key.view,
                    key.path.clone(),
                    super::RegistryKeyId32 {
                        slot: slot as u32,
                        generation: key.generation,
                    },
                    key.volatile,
                );
                if !volatile {
                    self.stage_change(
                        &mut prepared,
                        RegistryChangeKind32::SetValue,
                        hive,
                        view,
                        &path,
                        name,
                        Some(id),
                        modified,
                    )?;
                }
                if prepared.bytes > self.config.limits.bytes as usize {
                    return Err(E::NotEnoughMemory);
                }
                self.changes
                    .try_reserve(prepared.changes.len())
                    .map_err(|_| E::NotEnoughMemory)?;
                Ok(prepared)
            }
            Err(error) => Err(error),
        }
    }
    pub fn prepare_delete_path_value(
        &mut self,
        handle: u32,
        path: &[u16],
        name: &[u16],
        modified: u64,
    ) -> Result<PreparedRegistry32, E> {
        self.target(handle, 2)?;
        let (target, _) = self.resolved(handle, path, 0)?;
        self.prepare_delete_value_at(target.primary, name, modified)
    }
}
