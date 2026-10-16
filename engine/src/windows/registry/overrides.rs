use super::{Registry32, RegistryError32 as E};
impl Registry32 {
    pub fn override_predefined(&mut self, root: u32, new_handle: u32) -> Result<(), E> {
        let index = match root {
            0x80000000..=0x80000003 => root as usize - 0x80000000,
            0x80000005 => 5,
            _ => return Err(E::InvalidHandle),
        };
        let replacement = if new_handle == 0 {
            None
        } else {
            if new_handle & 0x80000000 != 0 {
                return Err(E::InvalidHandle);
            }
            let (target, view) = self.target(new_handle, 0)?;
            for id in std::iter::once(target.primary).chain(target.fallback) {
                self.key(id)?
                    .handles
                    .checked_add(1)
                    .ok_or(E::NotEnoughMemory)?;
            }
            Some((target, view))
        };
        self.revision.checked_add(1).ok_or(E::NotEnoughMemory)?;
        if let Some((target, _)) = replacement {
            for id in std::iter::once(target.primary).chain(target.fallback) {
                self.keys[id.slot as usize].as_mut().unwrap().handles += 1;
            }
        }
        if let Some((target, _)) = self.overrides[index] {
            for id in std::iter::once(target.primary).chain(target.fallback) {
                let key = self.keys[id.slot as usize].as_mut().unwrap();
                key.handles -= 1;
                if key.deleted && key.handles == 0 {
                    self.bytes -= key.bytes();
                    self.keys[id.slot as usize] = None;
                }
            }
        }
        self.overrides[index] = replacement;
        self.revision += 1;
        Ok(())
    }
    pub fn disable_predefined_cache(&mut self, all: bool) {
        self.disabled_cache |= if all { 3 } else { 1 };
    }
}
