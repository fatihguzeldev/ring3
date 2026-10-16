use super::{Enumeration32, EnumerationItem32};
use crate::{
    loader::{ResourceError32 as PeError, ResourceIdentifier32 as Identifier},
    memory::Access,
    process::{
        EngineInstance,
        windows::resources::{ResourceError32 as E, memory},
    },
    windows::resources::ResourceApi32 as Api,
};
pub(super) struct Selection32 {
    pub kind: Enumeration32,
    pub resource_type: Option<Identifier>,
    pub resource_name: Option<Identifier>,
    pub items: Vec<EnumerationItem32>,
    pub procedure: u32,
    pub parameter: u32,
}
impl EngineInstance {
    pub(super) fn select_resources(&self, api: Api, args: &[u32]) -> Result<Selection32, E> {
        let module = self.resource_module(args[0])?;
        let memory = self.memory.as_ref().ok_or(E::NotConfigured)?;
        let (kind, callback_index, extended) = match api {
            Api::EnumResourceTypesA | Api::EnumResourceTypesW => (Enumeration32::Types, 1, false),
            Api::EnumResourceTypesExA | Api::EnumResourceTypesExW => {
                (Enumeration32::Types, 1, true)
            }
            Api::EnumResourceNamesA | Api::EnumResourceNamesW => (Enumeration32::Names, 2, false),
            Api::EnumResourceNamesExA | Api::EnumResourceNamesExW => {
                (Enumeration32::Names, 2, true)
            }
            Api::EnumResourceLanguagesA | Api::EnumResourceLanguagesW => {
                (Enumeration32::Languages, 3, false)
            }
            Api::EnumResourceLanguagesExA | Api::EnumResourceLanguagesExW => {
                (Enumeration32::Languages, 3, true)
            }
            _ => return Err(E::InvalidParameter),
        };
        let flags = if extended {
            args[callback_index + 2]
        } else {
            0
        };
        if flags & !15 != 0 {
            return Err(E::InvalidParameter);
        }
        if args[callback_index] == 0 {
            return Err(E::InvalidParameter);
        }
        memory.check_span(
            crate::memory::GuestAddress(args[callback_index]),
            1,
            Access::Execute,
        )?;
        let resource_type = if matches!(kind, Enumeration32::Types) {
            None
        } else {
            Some(memory::identifier(memory, args[1], api.wide())?)
        };
        let resource_name = if matches!(kind, Enumeration32::Languages) {
            Some(memory::identifier(memory, args[2], api.wide())?)
        } else {
            None
        };
        let mut reader = self.resource_reader(module)?;
        let mut items = Vec::new();
        if flags == 0 || flags & 1 != 0 || flags & 3 == 0 {
            match kind {
                Enumeration32::Types => {
                    let values = reader.types()?;
                    items
                        .try_reserve_exact(values.len())
                        .map_err(|_| E::Allocation)?;
                    for value in values {
                        if flags & 8 != 0
                            && matches!(&value,Identifier::Name(name)if name.len()>260)
                        {
                            return Err(E::Pe(PeError::Malformed));
                        }
                        items.push(EnumerationItem32::Identifier(value));
                    }
                }
                Enumeration32::Names => {
                    let values = reader.names(resource_type.as_ref().unwrap())?;
                    items
                        .try_reserve_exact(values.len())
                        .map_err(|_| E::Allocation)?;
                    for value in values {
                        if flags & 8 != 0
                            && matches!(&value,Identifier::Name(name)if name.len()>260)
                        {
                            return Err(E::Pe(PeError::Malformed));
                        }
                        items.push(EnumerationItem32::Identifier(value));
                    }
                }
                Enumeration32::Languages => {
                    let values = reader.languages(
                        resource_type.as_ref().unwrap(),
                        resource_name.as_ref().unwrap(),
                    )?;
                    items
                        .try_reserve_exact(values.len())
                        .map_err(|_| E::Allocation)?;
                    items.extend(values.into_iter().map(EnumerationItem32::Language));
                }
            }
        }
        if items.is_empty() {
            return Err(E::Pe(match kind {
                Enumeration32::Types => PeError::TypeNotFound,
                Enumeration32::Names => PeError::NameNotFound,
                Enumeration32::Languages => PeError::LanguageNotFound,
            }));
        }
        Ok(Selection32 {
            kind,
            resource_type,
            resource_name,
            items,
            procedure: args[callback_index],
            parameter: args[callback_index + 1],
        })
    }
}
