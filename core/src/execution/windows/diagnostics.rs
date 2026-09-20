use super::{GuestMemory, MemoryError, PAGE_SIZE, PeImportSymbol, Permissions, ProcessStop};

const BASE: u32 = 0x7100_0000;
const MAX_IMPORTS: u32 = 4096;

#[derive(Default)]
pub(super) struct Imports {
    entries: Vec<(String, String)>,
}

impl Imports {
    pub(super) fn insert(&mut self, module: &str, symbol: PeImportSymbol<'_>) -> Option<u32> {
        let index = u32::try_from(self.entries.len()).ok()?;
        if index >= MAX_IMPORTS {
            return None;
        }
        self.entries.push((
            module.to_owned(),
            match symbol {
                PeImportSymbol::ByName { name, .. } => name.to_owned(),
                PeImportSymbol::Ordinal(value) => format!("#{value}"),
            },
        ));
        Some(BASE + index * 4)
    }

    pub(super) fn map(&self, memory: &mut GuestMemory) -> Result<(), MemoryError> {
        let length = (self.entries.len() as u64 * 4).div_ceil(PAGE_SIZE) * PAGE_SIZE;
        if length != 0 {
            memory.map_zeroed(u64::from(BASE), length, Permissions::NONE)?;
        }
        Ok(())
    }

    pub(super) fn contains(&self, address: u32) -> bool {
        self.index(address).is_some()
    }

    pub(super) fn stop(&self, address: u32) -> Option<ProcessStop> {
        let (module, symbol) = &self.entries[self.index(address)?];
        Some(ProcessStop::UnresolvedImport {
            address,
            module: module.clone(),
            symbol: symbol.clone(),
        })
    }

    fn index(&self, address: u32) -> Option<usize> {
        let offset = address.checked_sub(BASE)?;
        let index = usize::try_from(offset / 4).ok()?;
        (offset.is_multiple_of(4) && index < self.entries.len()).then_some(index)
    }
}
