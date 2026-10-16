use super::{PeResources32, ResourceError32 as E, ResourceIdentifier32};
impl PeResources32<'_> {
    fn identifiers(
        &mut self,
        table: super::directory::DirectoryTable32,
    ) -> Result<Vec<ResourceIdentifier32>, E> {
        let count = table.named + table.ids;
        let mut values = Vec::new();
        values
            .try_reserve_exact(count as usize)
            .map_err(|_| E::Allocation)?;
        for index in 0..count {
            let entry = self.entry(table, index)?;
            self.child(entry)?;
            values.push(self.entry_name(entry)?);
        }
        Ok(values)
    }
    pub fn types(&mut self) -> Result<Vec<ResourceIdentifier32>, E> {
        let table = self.table(0)?;
        self.identifiers(table)
    }
    pub fn names(&mut self, kind: &ResourceIdentifier32) -> Result<Vec<ResourceIdentifier32>, E> {
        let root = self.table(0)?;
        let entry = self.find_entry(root, kind)?.ok_or(E::TypeNotFound)?;
        let offset = self.child(entry)?;
        let table = self.table(offset)?;
        self.identifiers(table)
    }
    pub fn languages(
        &mut self,
        kind: &ResourceIdentifier32,
        name: &ResourceIdentifier32,
    ) -> Result<Vec<u16>, E> {
        let table = self.name_table(kind, name)?;
        if table.named != 0 {
            return Err(E::Malformed);
        }
        let mut values = Vec::new();
        values
            .try_reserve_exact(table.ids as usize)
            .map_err(|_| E::Allocation)?;
        for index in 0..table.ids {
            let entry = self.entry(table, index)?;
            if entry.name > u32::from(u16::MAX) || entry.target & 0x80000000 != 0 {
                return Err(E::Malformed);
            }
            self.location(self.directory_span(entry.target, 16)?)?;
            values.push(entry.name as u16);
        }
        Ok(values)
    }
}
