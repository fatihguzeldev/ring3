use crate::windows::registry::{
    PreparedRegistry32, Registry32, RegistryError32 as E, RegistryView32, model::Value32,
    prepared::Action32,
};
use crate::windows::text::ordinal_key;
pub struct RegistrySeed32<'a> {
    pub root: u32,
    pub view: RegistryView32,
    pub path: &'a [u16],
    pub class: &'a [u16],
    pub name: &'a [u16],
    pub kind: u32,
    pub bytes: &'a [u8],
    pub modified: u64,
    pub volatile: bool,
}
impl Registry32 {
    pub fn prepare_seed(&mut self, seed: RegistrySeed32<'_>) -> Result<PreparedRegistry32, E> {
        if seed.name.len() > 16383
            || seed.name.contains(&0)
            || seed.bytes.len() > self.config.limits.bytes as usize
        {
            return Err(E::InvalidParameter);
        }
        let flags = if seed.view == RegistryView32::View64 {
            0x100
        } else {
            0x200
        };
        match self.resolved(seed.root, seed.path, flags) {
            Ok((target, _)) => self.prepare_set_at(
                target.primary,
                seed.name,
                seed.kind,
                seed.bytes,
                seed.modified,
                false,
            ),
            Err(E::FileNotFound) => {
                let mut prepared = self.prepare_create_internal(
                    seed.root,
                    seed.path,
                    0xf003f | flags,
                    seed.class,
                    seed.volatile,
                    seed.modified,
                    false,
                    false,
                )?;
                let key = prepared
                    .actions
                    .iter_mut()
                    .rev()
                    .find_map(|action| match action {
                        Action32::Create(_, key) => Some(key),
                        _ => None,
                    })
                    .ok_or(E::InvalidData)?;
                let mut bytes = Vec::new();
                bytes
                    .try_reserve_exact(seed.bytes.len())
                    .map_err(|_| E::NotEnoughMemory)?;
                bytes.extend_from_slice(seed.bytes);
                let value = Value32 {
                    name: seed.name.to_vec(),
                    ordinal: ordinal_key(seed.name),
                    kind: seed.kind,
                    bytes,
                };
                prepared.bytes = prepared
                    .bytes
                    .checked_add(value.bytes())
                    .filter(|&bytes| bytes <= self.config.limits.bytes as usize)
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
                prepared.handle = None;
                prepared.next_handle = self.next_handle;
                prepared.result = 0;
                Ok(prepared)
            }
            Err(error) => Err(error),
        }
    }
}
