use super::{
    LoadConfig32, PreparedPe32,
    read::{ParseBudget, reserve},
};
use crate::loader::LoadError;

pub(super) fn parse(
    source: &[u8],
    image: &PreparedPe32,
    budget: &mut ParseBudget,
) -> Result<Option<LoadConfig32>, LoadError> {
    let Some(directory) = image.directories[10] else {
        return Ok(None);
    };
    if directory.size < 4 {
        return Err(LoadError::Malformed);
    }
    let declared_size = image.word(source, directory.rva)?;
    if declared_size < 4 || declared_size > directory.size {
        return Err(LoadError::Malformed);
    }
    let security_cookie_rva = if declared_size >= 64 {
        let va = image.word(source, directory.rva + 60)?;
        if va == 0 {
            None
        } else {
            Some(image.va_rva(va, 4)?)
        }
    } else {
        None
    };
    let mut seh_handlers = Vec::new();
    if declared_size >= 72 {
        let table = image.word(source, directory.rva + 64)?;
        let count = image.word(source, directory.rva + 68)?;
        if count != 0 {
            let bytes = count.checked_mul(4).ok_or(LoadError::Malformed)?;
            let table_rva = image.va_rva(table, bytes)?;
            budget.entries(count as usize)?;
            reserve(&mut seh_handlers, count as usize)?;
            for index in 0..count {
                let handler = image.word(source, table_rva + index * 4)?;
                image.mapped_span(handler, 1)?;
                if seh_handlers
                    .last()
                    .is_some_and(|previous| *previous >= handler)
                {
                    return Err(LoadError::Malformed);
                }
                seh_handlers.push(handler);
            }
        } else if table != 0 {
            image.va_rva(table, 0)?;
        }
    }
    let guard_flags = if declared_size >= 92 {
        Some(image.word(source, directory.rva + 88)?)
    } else {
        None
    };
    Ok(Some(LoadConfig32 {
        directory,
        declared_size,
        security_cookie_rva,
        seh_handlers,
        guard_flags,
    }))
}
