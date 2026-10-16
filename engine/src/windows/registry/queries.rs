use super::{
    Registry32, RegistryEntry32, RegistryError32 as E, RegistryKeyInfo32, RegistryNamedValue32,
    RegistryValue32,
    model::{Key32, Target32, Value32},
    path,
};
use crate::windows::text::ordinal_key;
impl Registry32 {
    pub(super) fn effective_target(&self, target: Target32) -> Result<Target32, E> {
        self.live_key(target.primary)?;
        if target.classes {
            let view = self.key(target.primary)?.view;
            self.classes_target(&self.relative_classes(target)?, view)
        } else {
            Ok(target)
        }
    }
    pub fn query_value(&self, handle: u32, name: &[u16]) -> Result<RegistryValue32<'_>, E> {
        let (target, _) = self.target(handle, 1)?;
        self.value_at(target, name)
    }
    pub fn query_subkey_value(
        &self,
        handle: u32,
        path: &[u16],
        flags: u32,
        name: &[u16],
    ) -> Result<RegistryValue32<'_>, E> {
        self.target(handle, 1)?;
        let (target, _) = self.resolved(handle, path, flags)?;
        self.value_at(target, name)
    }
    fn value_at(&self, target: Target32, name: &[u16]) -> Result<RegistryValue32<'_>, E> {
        if name.len() > 16383 || name.contains(&0) {
            return Err(E::InvalidParameter);
        }
        let target = self.effective_target(target)?;
        let ordinal = ordinal_key(name);
        for id in std::iter::once(target.primary).chain(target.fallback) {
            let key = self.live_key(id)?;
            if let Ok(at) = key
                .values
                .binary_search_by(|value| value.ordinal.cmp(&ordinal))
            {
                let value = &key.values[at];
                return Ok(RegistryValue32 {
                    kind: value.kind,
                    bytes: &value.bytes,
                });
            }
        }
        Err(E::FileNotFound)
    }
    pub(super) fn values_for(&self, target: Target32) -> Result<Vec<&Value32>, E> {
        let target = self.effective_target(target)?;
        let primary = self.live_key(target.primary)?;
        let fallback = target.fallback.map(|id| self.live_key(id)).transpose()?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(primary.values.len() + fallback.map_or(0, |key| key.values.len()))
            .map_err(|_| E::NotEnoughMemory)?;
        values.extend(primary.values.iter());
        if let Some(key) = fallback {
            values.extend(key.values.iter().filter(|candidate| {
                primary
                    .values
                    .binary_search_by(|value| value.ordinal.cmp(&candidate.ordinal))
                    .is_err()
            }));
        }
        values.sort_unstable_by(|a, b| a.ordinal.cmp(&b.ordinal));
        Ok(values)
    }
    pub(super) fn keys_for(&self, target: Target32) -> Result<Vec<(&Key32, usize)>, E> {
        let target = self.effective_target(target)?;
        let mut keys = Vec::new();
        for id in std::iter::once(target.primary).chain(target.fallback) {
            let parent = self.live_key(id)?;
            for child in &self.namespace {
                let key = self.live_key(*child)?;
                if key.hive == parent.hive
                    && key.view == parent.view
                    && path::immediate(&key.path, &parent.path).is_some()
                {
                    keys.try_reserve(1).map_err(|_| E::NotEnoughMemory)?;
                    keys.push((
                        key,
                        parent.path.len() + usize::from(!parent.path.is_empty()),
                    ));
                }
            }
        }
        keys.sort_by(|(a, at), (b, bt)| a.ordinal[*at..].cmp(&b.ordinal[*bt..]));
        keys.dedup_by(|(a, at), (b, bt)| a.ordinal[*at..] == b.ordinal[*bt..]);
        Ok(keys)
    }
    pub fn enumerate_key(&self, handle: u32, index: u32) -> Result<RegistryEntry32<'_>, E> {
        let (target, _) = self.target(handle, 8)?;
        let keys = self.keys_for(target)?;
        let (key, start) = keys.get(index as usize).copied().ok_or(E::NoMoreItems)?;
        Ok(RegistryEntry32 {
            name: &key.path[start..],
            class: &key.class,
            modified: key.modified,
        })
    }
    pub fn enumerate_value(&self, handle: u32, index: u32) -> Result<RegistryNamedValue32<'_>, E> {
        let (target, _) = self.target(handle, 1)?;
        let values = self.values_for(target)?;
        let value = values.get(index as usize).copied().ok_or(E::NoMoreItems)?;
        Ok(RegistryNamedValue32 {
            name: &value.name,
            value: RegistryValue32 {
                kind: value.kind,
                bytes: &value.bytes,
            },
        })
    }
    pub fn query_info(&self, handle: u32) -> Result<RegistryKeyInfo32<'_>, E> {
        self.query_info_encoded(handle, false)
    }
    pub fn query_info_encoded(&self, handle: u32, ansi: bool) -> Result<RegistryKeyInfo32<'_>, E> {
        let (target, _) = self.target(handle, 1)?;
        let target = self.effective_target(target)?;
        let key = self.live_key(target.primary)?;
        let keys = self.keys_for(target)?;
        let values = self.values_for(target)?;
        let length = |units: &[u16]| -> usize {
            if ansi {
                char::decode_utf16(units.iter().copied()).count()
            } else {
                units.len()
            }
        };
        Ok(RegistryKeyInfo32 {
            class: &key.class,
            modified: key.modified,
            subkeys: keys.len() as u32,
            maximum_subkey_name: keys
                .iter()
                .map(|(key, start)| length(&key.path[*start..]) as u32)
                .max()
                .unwrap_or(0),
            maximum_subkey_class: keys
                .iter()
                .map(|(key, _)| length(&key.class) as u32)
                .max()
                .unwrap_or(0),
            values: values.len() as u32,
            maximum_value_name: values
                .iter()
                .map(|value| length(&value.name) as u32)
                .max()
                .unwrap_or(0),
            maximum_value_bytes: values
                .iter()
                .map(|value| {
                    if ansi && matches!(value.kind, 1 | 2 | 7) {
                        let units: Vec<u16> = value
                            .bytes
                            .chunks_exact(2)
                            .map(|bytes| u16::from_le_bytes(bytes.try_into().unwrap()))
                            .collect();
                        length(&units) as u32
                    } else {
                        value.bytes.len() as u32
                    }
                })
                .max()
                .unwrap_or(0),
        })
    }
}
