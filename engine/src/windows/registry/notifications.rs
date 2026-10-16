use super::{
    PreparedRegistry32, Registry32, RegistryError32 as E, RegistryKeyId32 as Id,
    model::{Hive32, Key32, Target32},
    path,
    prepared::Action32,
};
#[derive(Clone, Copy)]
pub struct RegistryWatch32 {
    target: Target32,
    subtree: bool,
    filter: u32,
}
impl Registry32 {
    pub fn watch(&self, handle: u32, subtree: bool, filter: u32) -> Result<RegistryWatch32, E> {
        if filter & !0x1000000f != 0 || filter & 15 == 0 {
            return Err(E::InvalidParameter);
        }
        let (target, _) = self.target(handle, 16)?;
        Ok(RegistryWatch32 {
            target,
            subtree,
            filter,
        })
    }
    fn watch_key(&self, watch: RegistryWatch32, changed: &Key32, name: bool) -> bool {
        std::iter::once(watch.target.primary)
            .chain(watch.target.fallback)
            .any(|id| {
                let Ok(root) = self.key(id) else {
                    return false;
                };
                if root.view != changed.view {
                    return false;
                }
                let (root_path, changed_path) = if watch.target.classes {
                    let (Some(root), Some(changed)) =
                        (classes_relative(root), classes_relative(changed))
                    else {
                        return false;
                    };
                    (root, changed)
                } else {
                    if root.hive != changed.hive {
                        return false;
                    }
                    (&root.ordinal[..], &changed.ordinal[..])
                };
                if name {
                    if watch.subtree {
                        path::starts_with(changed_path, root_path)
                    } else {
                        path::immediate(changed_path, root_path).is_some()
                    }
                } else if watch.subtree {
                    path::starts_with(changed_path, root_path)
                } else {
                    changed_path == root_path
                }
            })
    }
    fn watch_path(
        &self,
        prepared: &PreparedRegistry32,
        watch: RegistryWatch32,
        id: Id,
        name: bool,
    ) -> bool {
        let key = self.key(id).ok().or_else(|| {
            prepared.actions.iter().find_map(|action| match action {
                Action32::Create(slot, key)
                    if *slot == id.slot as usize && key.generation == id.generation =>
                {
                    Some(key)
                }
                _ => None,
            })
        });
        key.is_some_and(|key| self.watch_key(watch, key, name))
    }
    pub fn notifies(&self, prepared: &PreparedRegistry32, watch: RegistryWatch32) -> bool {
        prepared.actions.iter().any(|action| match action {
            Action32::Create(_, key) => {
                watch.filter & 1 != 0 && self.watch_key(watch, key, true)
                    || watch.filter & 4 != 0
                        && !key.values.is_empty()
                        && self.watch_key(watch, key, false)
            }
            Action32::SetValue(id, _, _)
            | Action32::DeleteValue(id, _)
            | Action32::ReplaceValues(id, _, _)
                if watch.filter & 4 != 0 =>
            {
                self.watch_path(prepared, watch, *id, false)
            }
            Action32::DeleteKeys(ids) => ids.iter().any(|id| {
                *id == watch.target.primary
                    || watch.target.fallback == Some(*id)
                    || watch.filter & 1 != 0 && self.watch_path(prepared, watch, *id, true)
            }),
            _ => false,
        })
    }
}

fn classes_relative(key: &Key32) -> Option<&[u16]> {
    let prefix = match key.hive {
        Hive32::Users => path::USER_CLASSES,
        Hive32::Machine => path::MACHINE_CLASSES,
    };
    if key.ordinal.len() < prefix.len()
        || !key
            .ordinal
            .iter()
            .copied()
            .zip(prefix.bytes())
            .all(|(unit, byte)| unit == u16::from(byte.to_ascii_uppercase()))
    {
        return None;
    }
    if key.ordinal.len() == prefix.len() {
        Some(&[])
    } else if key.ordinal[prefix.len()] == 92 {
        Some(&key.ordinal[prefix.len() + 1..])
    } else {
        None
    }
}
