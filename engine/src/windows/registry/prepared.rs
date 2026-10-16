use super::{
    Registry32, RegistryError32 as E, RegistryKeyId32 as Id,
    model::{Change32, Handle32, Key32, Value32},
};
pub(super) enum Action32 {
    Create(usize, Key32),
    SetValue(Id, usize, Value32),
    DeleteValue(Id, usize),
    DeleteKeys(Vec<Id>),
    Modified(Id, u64),
    ReplaceValues(Id, Vec<Value32>, u64),
}
pub struct PreparedRegistry32 {
    pub(super) revision: u64,
    pub(super) actions: Vec<Action32>,
    pub(super) changes: Vec<Change32>,
    pub(super) remove_changes: Vec<u64>,
    pub(super) handle: Option<(usize, Handle32)>,
    pub(super) next_key: u64,
    pub(super) next_handle: u32,
    pub(super) next_change: u64,
    pub(super) bytes: usize,
    pub(super) values: usize,
    pub(super) result: u32,
    pub(super) disposition: u32,
}
impl PreparedRegistry32 {
    pub fn result(&self) -> u32 {
        self.result
    }
    pub fn disposition(&self) -> u32 {
        self.disposition
    }
}
impl Registry32 {
    pub(super) fn preparation(&self) -> Result<PreparedRegistry32, E> {
        self.revision.checked_add(1).ok_or(E::NotEnoughMemory)?;
        Ok(PreparedRegistry32 {
            revision: self.revision,
            actions: Vec::new(),
            changes: Vec::new(),
            remove_changes: Vec::new(),
            handle: None,
            next_key: self.next_key,
            next_handle: self.next_handle,
            next_change: self.next_change,
            bytes: self.bytes,
            values: self.values,
            result: 0,
            disposition: 0,
        })
    }
    pub fn can_commit(&self, prepared: &PreparedRegistry32) -> Result<(), E> {
        if self.revision != prepared.revision {
            Err(E::StaleReceipt)
        } else {
            Ok(())
        }
    }
    pub fn commit(&mut self, prepared: PreparedRegistry32) -> u32 {
        assert_eq!(self.revision, prepared.revision);
        for action in prepared.actions {
            match action {
                Action32::Create(slot, key) => {
                    let id = Id {
                        slot: slot as u32,
                        generation: key.generation,
                    };
                    let hive = key.hive;
                    let view = key.view;
                    self.keys[slot] = Some(key);
                    let at = self
                        .namespace_position(hive, view, &self.key(id).unwrap().ordinal)
                        .unwrap_err();
                    self.namespace.insert(at, id);
                }
                Action32::SetValue(id, at, value) => {
                    let key = self.keys[id.slot as usize].as_mut().unwrap();
                    if key
                        .values
                        .get(at)
                        .is_some_and(|old| old.ordinal == value.ordinal)
                    {
                        key.values[at] = value;
                    } else {
                        key.values.insert(at, value);
                    }
                }
                Action32::DeleteValue(id, at) => {
                    self.keys[id.slot as usize]
                        .as_mut()
                        .unwrap()
                        .values
                        .remove(at);
                }
                Action32::ReplaceValues(id, values, time) => {
                    let key = self.keys[id.slot as usize].as_mut().unwrap();
                    key.values = values;
                    key.modified = time;
                }
                Action32::Modified(id, time) => {
                    self.keys[id.slot as usize].as_mut().unwrap().modified = time;
                }
                Action32::DeleteKeys(ids) => {
                    for id in ids {
                        self.namespace.retain(|entry| *entry != id);
                        let key = self.keys[id.slot as usize].as_mut().unwrap();
                        key.deleted = true;
                        key.values.clear();
                        if key.handles == 0 {
                            self.keys[id.slot as usize] = None;
                        }
                    }
                }
            }
        }
        self.changes
            .retain(|change| !prepared.remove_changes.contains(&change.id.sequence));
        self.changes.extend(prepared.changes);
        if let Some((slot, handle)) = prepared.handle {
            for id in std::iter::once(handle.target.primary).chain(handle.target.fallback) {
                self.keys[id.slot as usize].as_mut().unwrap().handles += 1;
            }
            self.handle_index.push((handle.value, slot));
            self.handles[slot] = Some(handle);
        }
        self.next_key = prepared.next_key;
        self.next_handle = prepared.next_handle;
        self.next_change = prepared.next_change;
        self.bytes = prepared.bytes;
        self.values = prepared.values;
        self.revision += 1;
        prepared.result
    }
}
