use super::{
    ExportTarget32, Exports32, PreparedPe32,
    read::{ParseBudget, reserve},
};
use crate::loader::LoadError;

pub(super) fn parse(
    source: &[u8],
    image: &PreparedPe32,
    budget: &mut ParseBudget,
) -> Result<Exports32, LoadError> {
    let Some(directory) = image.directories[0] else {
        return Ok(Exports32::default());
    };
    if directory.size < 40 {
        return Err(LoadError::Malformed);
    }
    let at = directory.rva;
    let ordinal_base = image.word(source, at + 16)?;
    let function_count = image.word(source, at + 20)?;
    let name_count = image.word(source, at + 24)?;
    let functions = image.word(source, at + 28)?;
    let names_at = image.word(source, at + 32)?;
    let ordinals = image.word(source, at + 36)?;
    if (function_count != 0 && functions == 0)
        || (name_count != 0 && (names_at == 0 || ordinals == 0))
    {
        return Err(LoadError::Malformed);
    }
    if u64::from(ordinal_base) + u64::from(function_count) > 1 << 32 {
        return Err(LoadError::Malformed);
    }
    let functions_size = function_count.checked_mul(4).ok_or(LoadError::Malformed)?;
    let names_size = name_count.checked_mul(4).ok_or(LoadError::Malformed)?;
    let ordinals_size = name_count.checked_mul(2).ok_or(LoadError::Malformed)?;
    if function_count != 0 {
        image.mapped_span(functions, functions_size)?;
    }
    if name_count != 0 {
        image.mapped_span(names_at, names_size)?;
        image.mapped_span(ordinals, ordinals_size)?;
    }
    budget.entries(function_count as usize)?;
    budget.entries(name_count as usize)?;
    let mut targets = Vec::new();
    let mut names = Vec::new();
    reserve(&mut targets, function_count as usize)?;
    reserve(&mut names, name_count as usize)?;
    let end = directory.rva + directory.size;
    for index in 0..function_count {
        let rva = image.word(source, functions + index * 4)?;
        let target = if rva == 0 {
            None
        } else if rva >= directory.rva && rva < end {
            Some(ExportTarget32::Forwarder(
                image.string(source, rva, end, budget)?,
            ))
        } else {
            image.mapped_span(rva, 1)?;
            Some(ExportTarget32::Address(rva))
        };
        targets.push(target);
    }
    for index in 0..name_count {
        let rva = image.word(source, names_at + index * 4)?;
        let ordinal = u16::from_le_bytes(image.read(source, ordinals + index * 2)?);
        if u32::from(ordinal) >= function_count {
            return Err(LoadError::Malformed);
        }
        names.push((
            image.string(source, rva, image.image_size, budget)?,
            u32::from(ordinal),
        ));
    }
    if names.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
        return Err(LoadError::Malformed);
    }
    Ok(Exports32 {
        ordinal_base,
        targets,
        names,
    })
}
