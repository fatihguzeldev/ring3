use super::{
    ImportModule32, ImportSlot32, ImportSymbol32, PreparedPe32,
    read::{ParseBudget, reserve},
};
use crate::loader::LoadError;

pub(super) fn parse(
    source: &[u8],
    image: &PreparedPe32,
    budget: &mut ParseBudget,
) -> Result<Vec<ImportModule32>, LoadError> {
    let Some(directory) = image.directories[1] else {
        return Ok(Vec::new());
    };
    let mut modules = Vec::new();
    let mut slots = Vec::new();
    let mut terminated = false;
    for offset in (0..directory.size.saturating_sub(19)).step_by(20) {
        let at = directory.rva + offset;
        let record = image.read::<20>(source, at)?;
        if record.iter().all(|byte| *byte == 0) {
            terminated = true;
            break;
        }
        budget.entries(1)?;
        let lookup = image.word(source, at)?;
        let module_name = image.word(source, at + 12)?;
        let iat = image.word(source, at + 16)?;
        if iat == 0 {
            return Err(LoadError::Malformed);
        }
        let name = image.string(source, module_name, image.image_size, budget)?;
        let mut imports = Vec::new();
        let mut lookup_rva = if lookup == 0 { iat } else { lookup };
        let mut iat_rva = iat;
        loop {
            let value = image.word(source, lookup_rva)?;
            image.mapped_span(iat_rva, 4)?;
            if value == 0 {
                break;
            }
            budget.entries(1)?;
            let symbol = if value & 0x8000_0000 != 0 {
                if value & 0x7fff_0000 != 0 {
                    return Err(LoadError::Malformed);
                }
                ImportSymbol32::Ordinal(value as u16)
            } else {
                image.read::<2>(source, value)?;
                let name_rva = value.checked_add(2).ok_or(LoadError::Malformed)?;
                ImportSymbol32::Name(image.string(source, name_rva, image.image_size, budget)?)
            };
            reserve(&mut imports, 1)?;
            reserve(&mut slots, 1)?;
            imports.push(ImportSlot32 { iat_rva, symbol });
            slots.push(iat_rva);
            lookup_rva = lookup_rva.checked_add(4).ok_or(LoadError::Malformed)?;
            iat_rva = iat_rva.checked_add(4).ok_or(LoadError::Malformed)?;
        }
        reserve(&mut modules, 1)?;
        modules.push(ImportModule32 {
            name,
            slots: imports,
        });
    }
    if !terminated {
        return Err(LoadError::Malformed);
    }
    slots.sort_unstable();
    if slots
        .windows(2)
        .any(|pair| u64::from(pair[0]) + 4 > u64::from(pair[1]))
    {
        return Err(LoadError::Malformed);
    }
    Ok(modules)
}
