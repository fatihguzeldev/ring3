use super::ExportValue32;
use crate::{
    loader::{ImportSymbol32, LoadError, LoadedModule32, ModuleError32, ResolvedExport32},
    memory::{Access, AddressSpace, GuestAddress},
};

struct ExportReader<'a> {
    memory: &'a AddressSpace,
    module: &'a LoadedModule32,
}

impl ExportReader<'_> {
    fn span(&self, rva: u32, length: usize) -> Result<GuestAddress, ModuleError32> {
        if u64::from(rva) + length as u64 > u64::from(self.module.image_size()) {
            return Err(ModuleError32::Pe(LoadError::Malformed));
        }
        let address = GuestAddress(self.module.base() + rva);
        self.memory.check_span(address, length, Access::Read)?;
        Ok(address)
    }

    fn read<const N: usize>(&self, rva: u32) -> Result<[u8; N], ModuleError32> {
        let address = self.span(rva, N)?;
        let mut bytes = [0; N];
        self.memory.read(address, &mut bytes)?;
        Ok(bytes)
    }

    fn word(&self, rva: u32) -> Result<u32, ModuleError32> {
        Ok(u32::from_le_bytes(self.read(rva)?))
    }

    fn string(&self, rva: u32, end: u32, budget: &mut usize) -> Result<String, ModuleError32> {
        let mut bytes = Vec::new();
        let mut cursor = rva;
        while cursor < end {
            let length = (4096 - ((self.module.base() + cursor) & 4095)).min(end - cursor) as usize;
            let address = self.span(cursor, length)?;
            let mut page = [0; 4096];
            self.memory.read(address, &mut page[..length])?;
            let count = page[..length]
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(length);
            if count > *budget {
                return Err(ModuleError32::Capacity);
            }
            *budget -= count;
            if page[..count]
                .iter()
                .any(|byte| !(0x20..=0x7e).contains(byte))
            {
                return Err(ModuleError32::Pe(LoadError::Malformed));
            }
            bytes
                .try_reserve(count)
                .map_err(|_| ModuleError32::Allocation)?;
            bytes.extend_from_slice(&page[..count]);
            if count < length {
                if bytes.is_empty() {
                    return Err(ModuleError32::Pe(LoadError::Malformed));
                }
                return String::from_utf8(bytes)
                    .map_err(|_| ModuleError32::Pe(LoadError::Malformed));
            }
            cursor += length as u32;
        }
        Err(ModuleError32::Pe(LoadError::Malformed))
    }

    fn compare_string(
        &self,
        rva: u32,
        expected: &str,
        budget: &mut usize,
    ) -> Result<std::cmp::Ordering, ModuleError32> {
        let mut cursor = rva;
        let mut compared = 0;
        while cursor < self.module.image_size() {
            let length = (4096 - ((self.module.base() + cursor) & 4095))
                .min(self.module.image_size() - cursor) as usize;
            let address = self.span(cursor, length)?;
            let mut page = [0; 4096];
            self.memory.read(address, &mut page[..length])?;
            for byte in &page[..length] {
                if *byte == 0 {
                    return Ok(if compared == expected.len() {
                        std::cmp::Ordering::Equal
                    } else {
                        std::cmp::Ordering::Less
                    });
                }
                if *budget == 0 {
                    return Err(ModuleError32::Capacity);
                }
                *budget -= 1;
                if !(0x20..=0x7e).contains(byte) {
                    return Err(ModuleError32::Pe(LoadError::Malformed));
                }
                let Some(expected) = expected.as_bytes().get(compared) else {
                    return Ok(std::cmp::Ordering::Greater);
                };
                match byte.cmp(expected) {
                    std::cmp::Ordering::Equal => compared += 1,
                    order => return Ok(order),
                }
            }
            cursor += length as u32;
        }
        Err(ModuleError32::Pe(LoadError::Malformed))
    }

    fn table(&self, rva: u32, count: u32, width: u32) -> Result<(), ModuleError32> {
        let length = u64::from(count) * u64::from(width);
        if rva == 0 || length > usize::MAX as u64 {
            return Err(ModuleError32::Pe(LoadError::Malformed));
        }
        self.span(rva, length as usize)?;
        Ok(())
    }
}

pub(super) fn lookup(
    memory: &AddressSpace,
    module: &LoadedModule32,
    symbol: &ImportSymbol32,
) -> Result<ExportValue32, ModuleError32> {
    let reader = ExportReader { memory, module };
    let dos = reader.read::<64>(0)?;
    if dos[..2] != *b"MZ" {
        return Err(ModuleError32::Pe(LoadError::Malformed));
    }
    let pe = u32::from_le_bytes(dos[0x3c..0x40].try_into().unwrap());
    let coff = reader.read::<24>(pe)?;
    if coff[..4] != *b"PE\0\0"
        || u16::from_le_bytes(coff[4..6].try_into().unwrap()) != 0x14c
        || u16::from_le_bytes(coff[20..22].try_into().unwrap()) < 104
    {
        return Err(ModuleError32::Pe(LoadError::Malformed));
    }
    let optional = pe
        .checked_add(24)
        .ok_or(ModuleError32::Pe(LoadError::Malformed))?;
    let header = reader.read::<104>(optional)?;
    if u16::from_le_bytes(header[..2].try_into().unwrap()) != 0x10b {
        return Err(ModuleError32::Pe(LoadError::Malformed));
    }
    if u32::from_le_bytes(header[92..96].try_into().unwrap()) == 0 {
        return Err(ModuleError32::MissingSymbol);
    }
    let directory = u32::from_le_bytes(header[96..100].try_into().unwrap());
    let size = u32::from_le_bytes(header[100..104].try_into().unwrap());
    if directory == 0 && size == 0 {
        return Err(ModuleError32::MissingSymbol);
    }
    if directory == 0
        || size < 40
        || u64::from(directory) + u64::from(size) > u64::from(module.image_size())
    {
        return Err(ModuleError32::Pe(LoadError::Malformed));
    }
    let table = reader.read::<40>(directory)?;
    let field = |offset| u32::from_le_bytes(table[offset..offset + 4].try_into().unwrap());
    let ordinal_base = field(16);
    let functions = field(20);
    let names = field(24);
    let limits = module.parser_limits();
    if u64::from(functions) + u64::from(names) > limits.metadata_entries as u64 {
        return Err(ModuleError32::Capacity);
    }
    let mut string_budget = limits.string_bytes;
    let addresses_rva = field(28);
    let names_rva = field(32);
    let ordinals_rva = field(36);
    if functions == 0 {
        return Err(ModuleError32::MissingSymbol);
    }
    reader.table(addresses_rva, functions, 4)?;
    let index = match symbol {
        ImportSymbol32::Ordinal(ordinal) => u32::from(*ordinal)
            .checked_sub(ordinal_base)
            .filter(|index| *index < functions)
            .ok_or(ModuleError32::MissingSymbol)?,
        ImportSymbol32::Name(name) => {
            if names == 0 {
                return Err(ModuleError32::MissingSymbol);
            }
            reader.table(names_rva, names, 4)?;
            reader.table(ordinals_rva, names, 2)?;
            let mut low = 0;
            let mut high = names;
            let mut found = None;
            while low < high {
                let middle = low + (high - low) / 2;
                let candidate_rva = reader.word(names_rva + middle * 4)?;
                match reader.compare_string(candidate_rva, name, &mut string_budget)? {
                    std::cmp::Ordering::Less => low = middle + 1,
                    std::cmp::Ordering::Greater => high = middle,
                    std::cmp::Ordering::Equal => {
                        found = Some(u32::from(u16::from_le_bytes(
                            reader.read(ordinals_rva + middle * 2)?,
                        )));
                        break;
                    }
                }
            }
            let index = found.ok_or(ModuleError32::MissingSymbol)?;
            if index >= functions {
                return Err(ModuleError32::Pe(LoadError::Malformed));
            }
            index
        }
    };
    let target = reader.word(addresses_rva + index * 4)?;
    if target == 0 {
        return Err(ModuleError32::MissingSymbol);
    }
    if target >= directory && target < directory + size {
        return Ok(ExportValue32::Forwarder(reader.string(
            target,
            directory + size,
            &mut string_budget,
        )?));
    }
    if target >= module.image_size() {
        return Err(ModuleError32::Pe(LoadError::Malformed));
    }
    let address = GuestAddress(module.base() + target);
    memory.permissions_at(address)?;
    Ok(ExportValue32::Resolved(ResolvedExport32::GuestAddress {
        module: module.id(),
        address,
    }))
}
