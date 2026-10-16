use super::{E, arguments::string_error};
use crate::{
    loader::{ResourceError32 as PeError, ResourceIdentifier32 as Id},
    process::{EngineInstance, windows::strings::Input32},
    windows::text::messages::{message_table_entry, system_message},
};

impl EngineInstance {
    pub(super) fn message_source(
        &self,
        flags: u32,
        source: u32,
        id: u32,
        language: u32,
        wide: bool,
    ) -> Result<Vec<u16>, E> {
        if flags & 0x400 != 0 {
            return Input32::read(
                self.memory.as_ref().ok_or(E::NotConfigured)?,
                source,
                wide,
                self.windows_ansi_code_page(),
                65536,
            )
            .map(|input| input.units)
            .map_err(|error| E::from(string_error(error)));
        }
        if language > u32::from(u16::MAX) {
            return Err(E::InvalidParameter);
        }
        if flags & 0x800 != 0 {
            match self.module_message(source, id, language as u16) {
                Ok(units) => return Ok(units),
                Err(E::Pe(
                    PeError::TypeNotFound | PeError::NameNotFound | PeError::LanguageNotFound,
                ))
                | Err(E::Win32(317))
                    if flags & 0x1000 != 0 => {}
                Err(error) => return Err(error),
            }
        }
        if flags & 0x1000 == 0 {
            return Err(E::Win32(317));
        }
        if ![0, 9, 0x409].contains(&language) {
            return Err(E::Win32(1815));
        }
        let id = if id & 0xffff0000 == 0x80070000 {
            id & 0xffff
        } else {
            id
        };
        let message = system_message(id).ok_or(E::Win32(317))?;
        let mut units = Vec::new();
        units
            .try_reserve_exact(message.len())
            .map_err(|_| E::Allocation)?;
        units.extend(message.encode_utf16());
        Ok(units)
    }
    fn module_message(&self, module: u32, id: u32, language: u16) -> Result<Vec<u16>, E> {
        let module = self.resource_module(module)?;
        let mut reader = self.resource_reader(module)?;
        let names = reader.names(&Id::Id(11))?;
        let mut missing_language = false;
        for name in names {
            let location = if language != 0 {
                reader.find_exact(&Id::Id(11), &name, language)
            } else {
                reader.find_languages(
                    &Id::Id(11),
                    &name,
                    &[
                        0,
                        self.windows_thread_locale() as u16,
                        self.windows_user_locale() as u16,
                        0x409,
                    ],
                    true,
                )
            };
            let location = match location {
                Ok(location) => location,
                Err(PeError::LanguageNotFound) => {
                    missing_language = true;
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            if location.size > 1024 * 1024 {
                return Err(E::Capacity);
            }
            let address = reader.address(location.data_rva, location.size)?;
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(location.size as usize)
                .map_err(|_| E::Allocation)?;
            bytes.resize(location.size as usize, 0);
            self.memory
                .as_ref()
                .ok_or(E::NotConfigured)?
                .read(address, &mut bytes)?;
            if let Some(entry) = message_table_entry(&bytes, id)? {
                if entry.wide {
                    let mut units = Vec::new();
                    units
                        .try_reserve_exact(entry.bytes.len() / 2)
                        .map_err(|_| E::Allocation)?;
                    units.extend(
                        entry
                            .bytes
                            .chunks_exact(2)
                            .map(|b| u16::from_le_bytes([b[0], b[1]])),
                    );
                    return Ok(units);
                }
                return self
                    .windows_ansi_code_page()
                    .decode(entry.bytes, false)
                    .map_err(|_| E::InvalidParameter);
            }
        }
        Err(E::Win32(if missing_language { 1815 } else { 317 }))
    }
}
