use crate::windows::registry::{
    Registry32, RegistryChange32, RegistryChangeId32, RegistryChangeKind32 as Kind,
    RegistryError32 as E, RegistryValue32,
};
use crate::windows::text::ordinal_key;
impl Registry32 {
    pub fn change(&self, index: u32) -> Result<Option<RegistryChange32<'_>>, E> {
        let Some(change) = self.changes.get(index as usize) else {
            return Ok(None);
        };
        let key = change.key.map(|id| self.key(id)).transpose()?;
        let value = if change.kind == Kind::SetValue {
            let key = key.ok_or(E::StaleReceipt)?;
            let at = key
                .values
                .binary_search_by(|value| value.ordinal.cmp(&ordinal_key(&change.name)))
                .map_err(|_| E::StaleReceipt)?;
            let value = &key.values[at];
            Some(RegistryValue32 {
                kind: value.kind,
                bytes: &value.bytes,
            })
        } else {
            None
        };
        Ok(Some(RegistryChange32 {
            id: change.id,
            kind: change.kind,
            root: change.hive.handle(),
            view: change.view,
            path: &change.path,
            name: &change.name,
            class: key.map_or(&[], |key| key.class.as_slice()),
            modified: change.modified,
            value,
        }))
    }
    pub fn read_change(
        &self,
        id: RegistryChangeId32,
        offset: u32,
        output: &mut [u8],
    ) -> Result<usize, E> {
        let index = self
            .changes
            .iter()
            .position(|change| change.id == id)
            .ok_or(E::StaleReceipt)?;
        let value = self.change(index as u32)?.ok_or(E::StaleReceipt)?;
        let bytes = value.value.map_or(&[][..], |value| value.bytes);
        let start = offset as usize;
        if start > bytes.len() {
            return Err(E::InvalidParameter);
        }
        let length = output.len().min(bytes.len() - start);
        output[..length].copy_from_slice(&bytes[start..start + length]);
        Ok(length)
    }
    pub fn read_change_text(
        &self,
        id: RegistryChangeId32,
        part: u32,
        offset: u32,
        output: &mut [u16],
    ) -> Result<usize, E> {
        let index = self
            .changes
            .iter()
            .position(|change| change.id == id)
            .ok_or(E::StaleReceipt)?;
        let change = self.change(index as u32)?.ok_or(E::StaleReceipt)?;
        let text = match part {
            0 => change.path,
            1 => change.name,
            2 => change.class,
            _ => return Err(E::InvalidParameter),
        };
        let start = offset as usize;
        if start > text.len() {
            return Err(E::InvalidParameter);
        }
        let length = output.len().min(text.len() - start);
        output[..length].copy_from_slice(&text[start..start + length]);
        Ok(length)
    }
    pub fn acknowledge_change(&mut self, id: RegistryChangeId32) -> Result<(), E> {
        let index = self
            .changes
            .iter()
            .position(|change| change.id == id)
            .ok_or(E::StaleReceipt)?;
        self.revision.checked_add(1).ok_or(E::NotEnoughMemory)?;
        let change = self.changes.remove(index);
        self.bytes -= change.bytes();
        self.revision += 1;
        Ok(())
    }
    pub fn change_count(&self) -> usize {
        self.changes.len()
    }
}
