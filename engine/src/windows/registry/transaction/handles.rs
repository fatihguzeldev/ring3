use crate::windows::registry::{
    PreparedRegistry32, Registry32, RegistryError32 as E, RegistryView32 as View,
    model::{Handle32, Target32},
    path,
};

impl Registry32 {
    pub(in crate::windows::registry) fn prepared_handle(
        &mut self,
        prepared: &mut PreparedRegistry32,
        target: Target32,
        view: View,
        rights: u32,
    ) -> Result<(), E> {
        let slot = self
            .handles
            .iter()
            .position(Option::is_none)
            .ok_or(E::NotEnoughMemory)?;
        let value = prepared
            .next_handle
            .checked_add(4)
            .ok_or(E::NotEnoughMemory)?;
        for id in std::iter::once(target.primary).chain(target.fallback) {
            if let Ok(key) = self.key(id) {
                key.handles.checked_add(1).ok_or(E::NotEnoughMemory)?;
            }
        }
        self.handle_index
            .try_reserve(1)
            .map_err(|_| E::NotEnoughMemory)?;
        prepared.handle = Some((
            slot,
            Handle32 {
                value,
                target,
                access: rights,
                view,
            },
        ));
        prepared.next_handle = value;
        prepared.result = value;
        Ok(())
    }
    pub fn prepare_open(
        &mut self,
        handle: u32,
        subkey: &[u16],
        desired: u32,
    ) -> Result<PreparedRegistry32, E> {
        let rights = path::access(desired)?;
        let (target, view) = self.resolved(handle, subkey, desired)?;
        let mut prepared = self.preparation()?;
        if subkey.is_empty() && handle & 0x8000_0000 != 0 && desired & 0x300 == 0 {
            prepared.result = handle;
            return Ok(prepared);
        }
        self.prepared_handle(&mut prepared, target, view, rights)?;
        prepared.disposition = 2;
        Ok(prepared)
    }
    pub fn close(&mut self, handle: u32) -> Result<(), E> {
        if handle & 0x8000_0000 != 0 {
            self.root(handle, View::View32)?;
            return Ok(());
        }
        let slot = self.handle_slot(handle)?;
        self.revision.checked_add(1).ok_or(E::NotEnoughMemory)?;
        let handle = self.handles[slot].take().unwrap();
        for id in std::iter::once(handle.target.primary).chain(handle.target.fallback) {
            let key = self.keys[id.slot as usize].as_mut().unwrap();
            key.handles -= 1;
            if key.deleted && key.handles == 0 {
                self.bytes -= key.bytes();
                self.keys[id.slot as usize] = None;
            }
        }
        self.handle_index.retain(|(_, index)| *index != slot);
        self.revision += 1;
        Ok(())
    }
}
