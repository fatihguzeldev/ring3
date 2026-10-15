use super::{
    PreparedPe32,
    metadata::Relocation32,
    read::{ParseBudget, reserve},
};
use crate::loader::LoadError;

impl Relocation32 {
    pub(super) fn span(self) -> (u32, u32) {
        match self {
            Self::High(at) | Self::Low(at) | Self::HighAdj(at, _) => (at, 2),
            Self::HighLow(at) => (at, 4),
        }
    }
}

pub(super) fn parse(
    source: &[u8],
    image: &PreparedPe32,
    budget: &mut ParseBudget,
) -> Result<Vec<Relocation32>, LoadError> {
    let Some(directory) = image.relocation_directory else {
        return Ok(Vec::new());
    };
    if !directory.rva.is_multiple_of(4) {
        return Err(LoadError::Malformed);
    }
    let mut relocations = Vec::new();
    let mut offset = 0;
    while offset < directory.size {
        if !offset.is_multiple_of(4) || directory.size - offset < 8 {
            return Err(LoadError::Malformed);
        }
        let block = directory.rva + offset;
        let page = image.word(source, block)?;
        let size = image.word(source, block + 4)?;
        if !page.is_multiple_of(4096)
            || page >= image.image_size
            || size < 8
            || !size.is_multiple_of(2)
            || size > directory.size - offset
        {
            return Err(LoadError::Malformed);
        }
        budget.entries(1 + ((size - 8) / 2) as usize)?;
        let mut position = 8;
        while position < size {
            let entry = u16::from_le_bytes(image.read(source, block + position)?);
            position += 2;
            let kind = entry >> 12;
            if kind == 0 {
                continue;
            }
            let at = page
                .checked_add(u32::from(entry & 0xfff))
                .ok_or(LoadError::Malformed)?;
            let relocation = match kind {
                1 => Relocation32::High(at),
                2 => Relocation32::Low(at),
                3 => Relocation32::HighLow(at),
                4 => {
                    if position == size {
                        return Err(LoadError::Malformed);
                    }
                    let low = i16::from_le_bytes(image.read(source, block + position)?);
                    position += 2;
                    Relocation32::HighAdj(at, low)
                }
                _ => return Err(LoadError::Unsupported),
            };
            let (_, length) = relocation.span();
            image.mapped_span(at, length)?;
            if u64::from(at) < u64::from(directory.rva) + u64::from(directory.size)
                && u64::from(directory.rva) < u64::from(at) + u64::from(length)
            {
                return Err(LoadError::Malformed);
            }
            reserve(&mut relocations, 1)?;
            relocations.push(relocation);
        }
        offset += size;
    }
    relocations.sort_unstable_by_key(|relocation| relocation.span().0);
    if relocations.windows(2).any(|pair| {
        let (at, length) = pair[0].span();
        u64::from(at) + u64::from(length) > u64::from(pair[1].span().0)
    }) {
        return Err(LoadError::Malformed);
    }
    Ok(relocations)
}
