use super::{
    RegistryConfig32, RegistryError32 as E, RegistryKeyId32 as Id, RegistryView32 as View,
    model::{Change32, Handle32, Hive32, Key32, Target32},
    path::{self, CURRENT_CONFIG, MACHINE_CLASSES, USER_CLASSES, USER_SID},
};
use crate::windows::text::ordinal_key;
pub struct Registry32 {
    pub(super) epoch: u64,
    pub(super) config: RegistryConfig32,
    pub(super) revision: u64,
    pub(super) next_key: u64,
    pub(super) next_handle: u32,
    pub(super) next_change: u64,
    pub(super) keys: Vec<Option<Key32>>,
    pub(super) namespace: Vec<Id>,
    pub(super) handles: Vec<Option<Handle32>>,
    pub(super) handle_index: Vec<(u32, usize)>,
    pub(super) changes: Vec<Change32>,
    pub(super) bytes: usize,
    pub(super) values: usize,
    pub(super) overrides: [Option<(Target32, View)>; 6],
    pub(super) disabled_cache: u8,
}
impl Registry32 {
    pub fn new(epoch: u64, config: RegistryConfig32) -> Result<Self, E> {
        let limits = config.limits;
        if epoch == 0
            || !(16..=65536).contains(&limits.keys)
            || !(1..=262144).contains(&limits.values)
            || !(1..=65536).contains(&limits.handles)
            || !(4096..=256 * 1024 * 1024).contains(&limits.bytes)
            || !(1..=65536).contains(&limits.changes)
        {
            return Err(E::InvalidParameter);
        }
        let mut keys = Vec::new();
        keys.try_reserve_exact(limits.keys as usize)
            .map_err(|_| E::NotEnoughMemory)?;
        keys.resize_with(limits.keys as usize, || None);
        let mut handles = Vec::new();
        handles
            .try_reserve_exact(limits.handles as usize)
            .map_err(|_| E::NotEnoughMemory)?;
        handles.resize_with(limits.handles as usize, || None);
        let mut result = Self {
            epoch,
            config,
            revision: 1,
            next_key: 0,
            next_handle: 0x5300_0000,
            next_change: 0,
            keys,
            namespace: Vec::new(),
            handles,
            handle_index: Vec::new(),
            changes: Vec::new(),
            bytes: 0,
            values: 0,
            overrides: [None; 6],
            disabled_cache: 0,
        };
        for view in if config.separate_views {
            &[View::View32, View::View64][..]
        } else {
            &[View::View32][..]
        } {
            for (hive, name) in [
                (Hive32::Machine, ""),
                (Hive32::Users, ""),
                (Hive32::Users, USER_SID),
                (Hive32::Machine, "Software"),
                (Hive32::Machine, MACHINE_CLASSES),
                (Hive32::Users, &format!("{USER_SID}\\Software")),
                (Hive32::Users, USER_CLASSES),
                (Hive32::Machine, "SYSTEM"),
                (Hive32::Machine, "SYSTEM\\CurrentControlSet"),
                (
                    Hive32::Machine,
                    "SYSTEM\\CurrentControlSet\\Hardware Profiles",
                ),
                (Hive32::Machine, CURRENT_CONFIG),
            ] {
                result.insert_initial(hive, *view, name)?;
            }
        }
        Ok(result)
    }
    fn insert_initial(&mut self, hive: Hive32, view: View, name: &str) -> Result<(), E> {
        let slot = self
            .keys
            .iter()
            .position(Option::is_none)
            .ok_or(E::NotEnoughMemory)?;
        self.next_key += 1;
        let path: Vec<u16> = name.encode_utf16().collect();
        let ordinal = ordinal_key(&path);
        let key = Key32 {
            generation: self.next_key,
            hive,
            view,
            path,
            ordinal,
            class: Vec::new(),
            modified: 0,
            values: Vec::new(),
            deleted: false,
            handles: 0,
            volatile: false,
        };
        self.bytes += key.bytes();
        if self.bytes > self.config.limits.bytes as usize {
            return Err(E::NotEnoughMemory);
        }
        self.namespace
            .try_reserve(1)
            .map_err(|_| E::NotEnoughMemory)?;
        let id = Id {
            slot: slot as u32,
            generation: self.next_key,
        };
        self.keys[slot] = Some(key);
        let at = self
            .namespace_position(hive, view, &self.key(id)?.ordinal)
            .unwrap_err();
        self.namespace.insert(at, id);
        Ok(())
    }
    pub fn maximum_value_bytes(&self) -> usize {
        self.config.limits.bytes as usize
    }
    pub fn prepare_root_handle(
        &mut self,
        root: u32,
        desired: u32,
    ) -> Result<super::PreparedRegistry32, E> {
        let view = path::view(self.config.separate_views, desired, View::View32)?;
        let target = self.root(root, view)?;
        let rights = path::access(desired)?;
        let mut prepared = self.preparation()?;
        self.prepared_handle(&mut prepared, target, view, rights)?;
        Ok(prepared)
    }
    pub fn limits(&self) -> super::RegistryLimits32 { self.config.limits }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub(super) fn key(&self, id: Id) -> Result<&Key32, E> {
        self.keys
            .get(id.slot as usize)
            .and_then(Option::as_ref)
            .filter(|key| key.generation == id.generation)
            .ok_or(E::InvalidHandle)
    }
    pub(super) fn live_key(&self, id: Id) -> Result<&Key32, E> {
        let key = self.key(id)?;
        if key.deleted {
            Err(E::KeyDeleted)
        } else {
            Ok(key)
        }
    }
    pub(super) fn namespace_position(
        &self,
        hive: Hive32,
        view: View,
        ordinal: &[u16],
    ) -> Result<usize, usize> {
        self.namespace.binary_search_by(|id| {
            let key = self.key(*id).unwrap();
            (key.hive, key.view, key.ordinal.as_slice()).cmp(&(hive, view, ordinal))
        })
    }
    pub(super) fn lookup(&self, hive: Hive32, view: View, path: &[u16]) -> Option<Id> {
        self.namespace_position(hive, view, &ordinal_key(path))
            .ok()
            .map(|at| self.namespace[at])
    }
    pub(super) fn root(&self, handle: u32, view: View) -> Result<Target32, E> {
        if let Some((target, _)) = handle
            .checked_sub(0x80000000)
            .and_then(|index| self.overrides.get(index as usize))
            .copied()
            .flatten()
        {
            self.live_key(target.primary)?;
            return Ok(target);
        }

        if handle == 0x8000_0000 {
            return self.classes_target(&[], view);
        }
        let (hive, path) = match handle {
            0x8000_0001 => (Hive32::Users, USER_SID),
            0x8000_0002 => (Hive32::Machine, ""),
            0x8000_0003 => (Hive32::Users, ""),
            0x8000_0005 => (Hive32::Machine, CURRENT_CONFIG),
            _ => return Err(E::InvalidHandle),
        };
        let path: Vec<u16> = path.encode_utf16().collect();
        Ok(Target32 {
            primary: self.lookup(hive, view, &path).ok_or(E::FileNotFound)?,
            fallback: None,
            classes: false,
        })
    }
    pub(super) fn handle_slot(&self, handle: u32) -> Result<usize, E> {
        self.handle_index
            .binary_search_by_key(&handle, |(value, _)| *value)
            .ok()
            .map(|at| self.handle_index[at].1)
            .ok_or(E::InvalidHandle)
    }
    pub(super) fn target(&self, handle: u32, rights: u32) -> Result<(Target32, View), E> {
        if handle & 0x8000_0000 != 0 {
            let view = handle
                .checked_sub(0x80000000)
                .and_then(|index| self.overrides.get(index as usize))
                .copied()
                .flatten()
                .map_or(View::View32, |(_, view)| view);
            return self.root(handle, view).map(|target| (target, view));
        }
        let open = self.handles[self.handle_slot(handle)?].as_ref().unwrap();
        if open.access & rights != rights {
            return Err(E::AccessDenied);
        }
        self.live_key(open.target.primary)?;
        Ok((open.target, open.view))
    }
    pub(super) fn classes_target(&self, relative: &[u16], view: View) -> Result<Target32, E> {
        let user = path::joined(&USER_CLASSES.encode_utf16().collect::<Vec<_>>(), relative)?;
        let machine = path::joined(
            &MACHINE_CLASSES.encode_utf16().collect::<Vec<_>>(),
            relative,
        )?;
        match (
            self.lookup(Hive32::Users, view, &user),
            self.lookup(Hive32::Machine, view, &machine),
        ) {
            (Some(primary), fallback) => Ok(Target32 {
                primary,
                fallback,
                classes: true,
            }),
            (None, Some(primary)) => Ok(Target32 {
                primary,
                fallback: None,
                classes: true,
            }),
            _ => Err(E::FileNotFound),
        }
    }
    pub(super) fn relative_classes(&self, target: Target32) -> Result<Vec<u16>, E> {
        let key = self.live_key(target.primary)?;
        let prefix = if key.hive == Hive32::Users {
            USER_CLASSES
        } else {
            MACHINE_CLASSES
        };
        let length = prefix.encode_utf16().count();
        Ok(key
            .path
            .get(length + usize::from(key.path.len() > length)..)
            .unwrap_or(&[])
            .to_vec())
    }
    pub(super) fn resolved(
        &self,
        handle: u32,
        subkey: &[u16],
        flags: u32,
    ) -> Result<(Target32, View), E> {
        let (target, inherited) = self.target(handle, 0)?;
        let view = path::view(self.config.separate_views, flags, inherited)?;
        if target.classes {
            let relative = path::joined(&self.relative_classes(target)?, subkey)?;
            return self
                .classes_target(&relative, view)
                .map(|target| (target, view));
        }
        let base = self.live_key(target.primary)?;
        let path = path::joined(&base.path, subkey)?;
        let primary = self.lookup(base.hive, view, &path).ok_or(E::FileNotFound)?;
        Ok((
            Target32 {
                primary,
                fallback: None,
                classes: false,
            },
            view,
        ))
    }
    pub fn can_flush(&self, handle: u32) -> Result<bool, E> {
        let (target, _) = self.target(handle, 0)?;
        let target = self.effective_target(target)?;
        for id in std::iter::once(target.primary).chain(target.fallback) {
            let key = self.live_key(id)?;
            if self.changes.iter().any(|change| {
                change.hive == key.hive
                    && change.view == key.view
                    && path::starts_with(&ordinal_key(&change.path), &key.ordinal)
            }) {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
