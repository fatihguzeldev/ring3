use super::{
    PreparedPe32, StaticTls32,
    read::{ParseBudget, reserve},
};
use crate::{loader::LoadError, memory::Access};

pub(super) fn parse(
    source: &[u8],
    image: &PreparedPe32,
    budget: &mut ParseBudget,
) -> Result<Option<StaticTls32>, LoadError> {
    let Some(directory) = image.directories[9] else {
        return Ok(None);
    };
    if directory.size < 24 {
        return Err(LoadError::Malformed);
    }
    let at = directory.rva;
    let start = image.word(source, at)?;
    let end = image.word(source, at + 4)?;
    let index = image.word(source, at + 8)?;
    let callbacks = image.word(source, at + 12)?;
    let zero_fill = image.word(source, at + 16)?;
    let flags = image.word(source, at + 20)?;
    if end < start || index == 0 || flags & !0x00f0_0000 != 0 {
        return Err(LoadError::Malformed);
    }
    let alignment = (flags >> 20) & 15;
    if alignment == 15 {
        return Err(LoadError::Malformed);
    }
    let alignment = if alignment == 0 {
        4
    } else {
        1 << (alignment - 1)
    };
    let template_size = end - start;
    template_size
        .checked_add(zero_fill)
        .ok_or(LoadError::Malformed)?;
    let template_rva = if start == 0 && end == 0 {
        0
    } else {
        image.va_rva(start, template_size)?
    };
    let index_rva = image.va_rva(index, 4)?;
    let callbacks_rva = if callbacks == 0 {
        None
    } else {
        Some(image.va_rva(callbacks, 4)?)
    };
    let mut parsed_callbacks = Vec::new();
    if let Some(mut position) = callbacks_rva {
        loop {
            let callback = image.word(source, position)?;
            if callback == 0 {
                break;
            }
            let rva = image.va_rva(callback, 1)?;
            if !image.sections.iter().any(|section| {
                rva >= section.rva
                    && u64::from(rva) < u64::from(section.rva) + section.extent()
                    && section.permissions.allows(Access::Execute)
            }) {
                return Err(LoadError::Malformed);
            }
            budget.entries(1)?;
            reserve(&mut parsed_callbacks, 1)?;
            parsed_callbacks.push(rva);
            position = position.checked_add(4).ok_or(LoadError::Malformed)?;
        }
    }
    Ok(Some(StaticTls32 {
        template_rva,
        template_size,
        zero_fill,
        index_rva,
        callbacks: parsed_callbacks,
        callbacks_rva,
        alignment,
    }))
}
