use super::super::Access;
use super::{DispatchError, GuestMemory, MemoryError, guest, modules::Modules, thread};
use crate::execution::GuestModule;
use crate::execution::loader::modules::MappedModule;
use crate::{
    PeResourceDataEntryTable, PeResourceDirectory, PeResourceDirectoryEntry,
    PeResourcePayloadError, parse_pe_resource_payloads,
};

mod accelerators;
mod icons;

#[derive(Clone, Copy)]
pub(super) enum Call {
    Find,
    String,
    LoadAccelerators,
    CopyAccelerators,
    LoadIcon,
}

impl Call {
    pub(super) fn arguments(self) -> usize {
        match self {
            Self::Find | Self::CopyAccelerators => 3,
            Self::String => 4,
            Self::LoadAccelerators | Self::LoadIcon => 2,
        }
    }

    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0xe8 => Some(Self::Find),
            0xec => Some(Self::String),
            0x29c => Some(Self::LoadAccelerators),
            0x2a0 => Some(Self::CopyAccelerators),
            0x2c8 => Some(Self::LoadIcon),
            _ => None,
        }
    }
}

struct Image {
    base: u32,
    table: Result<Option<PeResourceDataEntryTable>, PeResourcePayloadError>,
}

pub(super) struct Resources {
    program: u32,
    images: Vec<Image>,
    accelerators: accelerators::Tables,
    icons: icons::Icons,
}

struct Resource {
    info: u32,
    base: u32,
    data: u32,
    size: u32,
}

enum Selection {
    Found(Resource),
    Missing(u32),
}

impl Resources {
    pub(super) fn new(
        program: u32,
        bytes: &[u8],
        providers: &[MappedModule],
        inputs: &[GuestModule<'_>],
    ) -> Self {
        let images = std::iter::once((program, bytes))
            .chain(
                providers
                    .iter()
                    .zip(inputs)
                    .map(|(provider, input)| (provider.base, input.bytes)),
            )
            .map(|(base, bytes)| Image {
                base,
                table: parse_pe_resource_payloads(bytes)
                    .map(|table| table.map(|table| table.data_entry_table)),
            })
            .collect();
        Self {
            program,
            images,
            accelerators: accelerators::Tables::default(),
            icons: icons::Icons::default(),
        }
    }

    pub(super) fn dispatch(
        &mut self,
        call: Call,
        arguments: &[u32],
        modules: &Modules,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        match call {
            Call::LoadIcon => self.load_icon(arguments, modules, memory),
            Call::LoadAccelerators => {
                if arguments[1] > 0xffff {
                    return Err(DispatchError::Unsupported);
                }
                let error = match self.find(arguments[0], 9, arguments[1], modules)? {
                    Selection::Found(resource) => {
                        if let Some(handle) = self.accelerators.load(&resource, memory)? {
                            return Ok(handle);
                        }
                        8
                    }
                    Selection::Missing(error) => error,
                };
                thread::set_last_error(memory, error)?;
                Ok(0)
            }
            Call::CopyAccelerators => self.accelerators.copy(arguments, memory),
            Call::Find => {
                let (name, kind) = (arguments[1], arguments[2]);
                if name > 0xffff || kind > 0xffff {
                    return Err(DispatchError::Unsupported);
                }
                match self.find(arguments[0], kind, name, modules)? {
                    Selection::Found(resource) => Ok(resource.info),
                    Selection::Missing(error) => {
                        thread::set_last_error(memory, error)?;
                        Ok(0)
                    }
                }
            }
            Call::String => {
                let (id, output, capacity) = (arguments[1], arguments[2], arguments[3]);
                if capacity.cast_signed() <= 0 {
                    return Err(DispatchError::Unsupported);
                }
                let selection = self.find(arguments[0], 6, ((id & 0xffff) >> 4) + 1, modules)?;
                let resource = match selection {
                    Selection::Found(resource) => resource,
                    Selection::Missing(error) => {
                        guest::check(memory, output, 1, Access::Write)?;
                        thread::check_last_error_write(memory)?;
                        memory.write(u64::from(output), &[0])?;
                        thread::set_last_error(memory, error)?;
                        return Ok(0);
                    }
                };
                let bytes = resource.string(memory, id & 15, capacity)?;
                guest::check(memory, output, bytes.len(), Access::Write)?;
                memory.write(u64::from(output), &bytes)?;
                Ok(u32::try_from(bytes.len() - 1).expect("resource string length fits u32"))
            }
        }
    }

    fn find(
        &self,
        module: u32,
        kind: u32,
        name: u32,
        modules: &Modules,
    ) -> Result<Selection, DispatchError> {
        let base = if module == 0 { self.program } else { module };
        if !modules.contains(base) {
            return Ok(Selection::Missing(87));
        }
        let Some(image) = self.images.iter().find(|image| image.base == base) else {
            return Ok(Selection::Missing(1812));
        };
        let Some(table) = image
            .table
            .as_ref()
            .map_err(|_| DispatchError::Unsupported)?
        else {
            return Ok(Selection::Missing(1812));
        };
        let graph = &table.directory_graph;
        let Some(kind_entry) = find_id(&graph.directories[0], kind)? else {
            return Ok(Selection::Missing(1813));
        };
        let names = kind_entry
            .child_directory_index
            .ok_or(DispatchError::Unsupported)?;
        let Some(name_entry) = find_id(&graph.directories[usize::from(names)], name)? else {
            return Ok(Selection::Missing(1814));
        };
        let languages = name_entry
            .child_directory_index
            .ok_or(DispatchError::Unsupported)?;
        let languages = &graph.directories[usize::from(languages)];
        if languages.number_of_named_entries != 0 {
            return Err(DispatchError::Unsupported);
        }
        let entries = id_entries(languages)?;
        if entries
            .iter()
            .any(|entry| entry.child_directory_index.is_some())
        {
            return Err(DispatchError::Unsupported);
        }
        let selected = [0, 0x409, 9]
            .into_iter()
            .find_map(|id| entries.iter().find(|entry| entry.raw_name_or_id == id))
            .or_else(|| entries.first());
        let Some(selected) = selected else {
            return Ok(Selection::Missing(1815));
        };
        let data = table
            .data_entries
            .iter()
            .find(|entry| entry.data_entry_offset == selected.raw_data_or_subdirectory)
            .ok_or(DispatchError::Unsupported)?;
        Ok(Selection::Found(Resource {
            info: base
                .checked_add(data.data_entry_rva.get())
                .ok_or(DispatchError::Unsupported)?,
            base,
            data: data.payload_rva.get(),
            size: data.payload_size,
        }))
    }
}

fn id_entries(
    directory: &PeResourceDirectory,
) -> Result<&[PeResourceDirectoryEntry], DispatchError> {
    let named = usize::from(directory.number_of_named_entries);
    if directory.entries[..named]
        .iter()
        .any(|entry| entry.raw_name_or_id & 0x8000_0000 == 0)
    {
        return Err(DispatchError::Unsupported);
    }
    let entries = &directory.entries[named..];
    if entries.iter().any(|entry| entry.raw_name_or_id > 0xffff)
        || entries
            .windows(2)
            .any(|pair| pair[0].raw_name_or_id >= pair[1].raw_name_or_id)
    {
        return Err(DispatchError::Unsupported);
    }
    Ok(entries)
}

fn find_id(
    directory: &PeResourceDirectory,
    id: u32,
) -> Result<Option<&PeResourceDirectoryEntry>, DispatchError> {
    Ok(id_entries(directory)?
        .iter()
        .find(|entry| entry.raw_name_or_id == id))
}

impl Resource {
    fn word(&self, memory: &GuestMemory, offset: u32) -> Result<u16, DispatchError> {
        if u64::from(offset) + 2 > u64::from(self.size) {
            return Err(DispatchError::Unsupported);
        }
        let address = self
            .base
            .checked_add(self.data)
            .and_then(|address| address.checked_add(offset))
            .ok_or(MemoryError::AddressOverflow)?;
        guest::check(memory, address, 2, Access::Read)?;
        let mut bytes = [0; 2];
        memory.read(u64::from(address), &mut bytes)?;
        Ok(u16::from_le_bytes(bytes))
    }

    fn string(
        &self,
        memory: &GuestMemory,
        slot: u32,
        capacity: u32,
    ) -> Result<Vec<u8>, DispatchError> {
        let mut offset = 0_u32;
        for index in 0..=slot {
            let length = u32::from(self.word(memory, offset)?);
            let start = offset.checked_add(2).ok_or(DispatchError::Unsupported)?;
            let end = start
                .checked_add(length * 2)
                .filter(|&end| end <= self.size)
                .ok_or(DispatchError::Unsupported)?;
            if index == slot {
                let count = length.min(capacity - 1);
                let mut bytes = Vec::new();
                for unit in 0..count {
                    let value = self.word(memory, start + unit * 2)?;
                    if value > 0x7f {
                        return Err(DispatchError::Unsupported);
                    }
                    bytes.push(u8::try_from(value).expect("ascii unit fits byte"));
                }
                bytes.push(0);
                return Ok(bytes);
            }
            offset = end;
        }
        unreachable!("slot is visited")
    }
}
