use super::{PeResources32, ResourceError32 as E, ResourceIdentifier32, ResourceLocation32};
impl PeResources32<'_> {
    pub(super) fn name_table(
        &mut self,
        kind: &ResourceIdentifier32,
        name: &ResourceIdentifier32,
    ) -> Result<super::directory::DirectoryTable32, E> {
        let root = self.table(0)?;
        let kind = self.find_entry(root, kind)?.ok_or(E::TypeNotFound)?;
        let offset = self.child(kind)?;
        let names = self.table(offset)?;
        let name = self.find_entry(names, name)?.ok_or(E::NameNotFound)?;
        let offset = self.child(name)?;
        self.table(offset)
    }
    pub fn location(&self, entry_rva: u32) -> Result<ResourceLocation32, E> {
        let offset = entry_rva
            .checked_sub(self.directory.rva)
            .ok_or(E::Malformed)?;
        if !offset.is_multiple_of(4) {
            return Err(E::Malformed);
        }
        self.directory_span(offset, 16)?;
        let bytes = self.read::<16>(entry_rva)?;
        if bytes[12..16] != [0; 4] {
            return Err(E::Malformed);
        }
        let location = ResourceLocation32 {
            entry_rva,
            data_rva: u32::from_le_bytes(bytes[..4].try_into().unwrap()),
            size: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            code_page: u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
        };
        self.image_span(location.data_rva, location.size)?;
        Ok(location)
    }
    pub fn find_exact(
        &mut self,
        kind: &ResourceIdentifier32,
        name: &ResourceIdentifier32,
        language: u16,
    ) -> Result<ResourceLocation32, E> {
        let table = self.name_table(kind, name)?;
        let entry = self
            .find_entry(table, &ResourceIdentifier32::Id(u32::from(language)))?
            .ok_or(E::LanguageNotFound)?;
        if entry.target & 0x80000000 != 0 {
            return Err(E::Malformed);
        }
        let rva = self.directory_span(entry.target, 16)?;
        self.location(rva)
    }
    pub fn find_languages(
        &mut self,
        kind: &ResourceIdentifier32,
        name: &ResourceIdentifier32,
        languages: &[u16],
        first_available: bool,
    ) -> Result<ResourceLocation32, E> {
        self.find_languages_with_language(kind, name, languages, first_available)
            .map(|(location, _)| location)
    }
    pub fn find_languages_with_language(
        &mut self,
        kind: &ResourceIdentifier32,
        name: &ResourceIdentifier32,
        languages: &[u16],
        first_available: bool,
    ) -> Result<(ResourceLocation32, u16), E> {
        let table = self.name_table(kind, name)?;
        for &language in languages {
            if let Some(entry) =
                self.find_entry(table, &ResourceIdentifier32::Id(u32::from(language)))?
            {
                if entry.target & 0x80000000 != 0 {
                    return Err(E::Malformed);
                }
                return Ok((
                    self.location(self.directory_span(entry.target, 16)?)?,
                    language,
                ));
            }
        }
        if first_available && table.ids != 0 {
            let entry = self.entry(table, table.named)?;
            if entry.target & 0x80000000 != 0 || entry.name > u32::from(u16::MAX) {
                return Err(E::Malformed);
            }
            return Ok((
                self.location(self.directory_span(entry.target, 16)?)?,
                entry.name as u16,
            ));
        }
        Err(E::LanguageNotFound)
    }
}
