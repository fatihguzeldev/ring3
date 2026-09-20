use super::{GuestMemory, LoadError, PeImportSymbol};
use crate::{PeFileRangeSource, parse_pe_import_lookups, resolve_pe_file_range};

pub(super) fn bind(
    bytes: &[u8],
    base: u64,
    memory: &mut GuestMemory,
    mut resolver: impl FnMut(&str, PeImportSymbol<'_>) -> Result<Option<u32>, LoadError>,
) -> Result<(), LoadError> {
    let imports = parse_pe_import_lookups(bytes).map_err(LoadError::Imports)?;
    let mut ranges = Vec::new();
    for import in imports {
        let descriptor = import.descriptor;
        let rva = descriptor.import_address_table_rva;
        let count = u32::try_from(import.entries.len())
            .map_err(|_| LoadError::InvalidImportAddressTable)?;
        let length = (count + 1) * 4;
        let end = u64::from(rva.get()) + u64::from(length);
        let range = resolve_pe_file_range(bytes, rva, length)
            .map_err(|_| LoadError::InvalidImportAddressTable)?;
        if !rva.get().is_multiple_of(4)
            || !matches!(range.source, PeFileRangeSource::Section(_))
            || range.bytes[range.bytes.len() - 4..] != [0; 4]
            || ranges
                .iter()
                .any(|&(start, prior_end)| u64::from(rva.get()) < prior_end && start < end)
        {
            return Err(LoadError::InvalidImportAddressTable);
        }
        ranges.push((u64::from(rva.get()), end));
        for (index, entry) in (0_u64..).zip(import.entries) {
            let address = resolver(descriptor.dll_name, entry.symbol)?.ok_or_else(|| {
                LoadError::UnresolvedImport {
                    module: descriptor.dll_name.to_owned(),
                    symbol: match entry.symbol {
                        PeImportSymbol::ByName { name, .. } => name.to_owned(),
                        PeImportSymbol::Ordinal(value) => format!("#{value}"),
                    },
                }
            })?;
            memory.write(
                base + u64::from(rva.get()) + index * 4,
                &address.to_le_bytes(),
            )?;
        }
    }
    Ok(())
}
