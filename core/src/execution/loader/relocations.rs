use super::{GuestMemory, Image, LoadError, PAGE_SIZE};
use crate::parse_pe_base_relocation_blocks;

pub(super) fn apply(image: &Image<'_>, memory: &mut GuestMemory) -> Result<(), LoadError> {
    let preferred = image.table.headers.optional.image_base;
    if image.base() == preferred {
        return Ok(());
    }
    if image.table.headers.prefix.characteristics & 1 != 0 {
        return Err(LoadError::RelocationRequired);
    }
    let blocks = parse_pe_base_relocation_blocks(image.bytes).map_err(LoadError::Relocations)?;
    if blocks.is_empty() {
        return Err(LoadError::RelocationRequired);
    }
    let delta = u32::try_from(image.base())
        .expect("validated actual base")
        .wrapping_sub(u32::try_from(preferred).expect("validated preferred base"));
    for block in blocks {
        let page = block.page_rva.get();
        if !u64::from(page).is_multiple_of(PAGE_SIZE) {
            return Err(LoadError::UnalignedRelocationPage { rva: page });
        }
        for bytes in block.raw_entries.chunks_exact(2) {
            let entry = u16::from_le_bytes([bytes[0], bytes[1]]);
            match entry >> 12 {
                0 => continue,
                3 => {}
                kind => return Err(LoadError::UnsupportedRelocation { kind }),
            }
            let rva = u64::from(page) + u64::from(entry & 0xfff);
            if rva + 4 > u64::from(image.table.headers.optional.size_of_image) {
                return Err(LoadError::RelocationTarget { rva });
            }
            let address = image.base() + rva;
            let mut value = [0; 4];
            memory.read(address, &mut value)?;
            let relocated = u32::from_le_bytes(value).wrapping_add(delta);
            memory.write(address, &relocated.to_le_bytes())?;
        }
    }
    Ok(())
}
