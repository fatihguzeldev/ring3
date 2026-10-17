use super::{
    Directory32, Exports32, ImageKind32, PreparedPe32, Section32,
    read::{ParseBudget, align, reserve, u16, u32},
};
use crate::{loader::LoadError, memory::Permissions};

pub(super) fn parse(bytes: &[u8], budget: &mut ParseBudget) -> Result<PreparedPe32, LoadError> {
    if bytes.get(..2) != Some(b"MZ") || bytes.len() < 64 {
        return Err(LoadError::Malformed);
    }
    let pe = u32(bytes, 0x3c)? as usize;
    let coff = pe.checked_add(4).ok_or(LoadError::Malformed)?;
    let optional = coff.checked_add(20).ok_or(LoadError::Malformed)?;
    if pe < 64 || bytes.get(pe..coff) != Some(b"PE\0\0") || optional > bytes.len() {
        return Err(LoadError::Malformed);
    }
    if u16(bytes, coff)? != 0x14c {
        return Err(LoadError::Unsupported);
    }
    let count = usize::from(u16(bytes, coff + 2)?);
    if count == 0 {
        return Err(LoadError::Malformed);
    }
    if count > 96 {
        return Err(LoadError::Unsupported);
    }
    let optional_size = usize::from(u16(bytes, coff + 16)?);
    let table = optional
        .checked_add(optional_size)
        .ok_or(LoadError::Malformed)?;
    let table_end = table.checked_add(count * 40).ok_or(LoadError::Malformed)?;
    if optional_size < 96 || table_end > bytes.len() {
        return Err(LoadError::Malformed);
    }
    if u16(bytes, optional)? != 0x10b {
        return Err(LoadError::Unsupported);
    }
    let characteristics = u16(bytes, coff + 18)?;
    if characteristics & 2 == 0 || characteristics & 0x9080 != 0 {
        return Err(LoadError::Unsupported);
    }
    if characteristics & 0x40 != 0 {
        return Err(LoadError::Malformed);
    }
    let dll_characteristics = u16(bytes, optional + 70)?;
    if dll_characteristics & 0x3080 != 0 {
        return Err(LoadError::Unsupported);
    }
    if dll_characteristics & 0x000f != 0 {
        return Err(LoadError::Malformed);
    }
    if !matches!(u16(bytes, optional + 68)?, 2 | 3) {
        return Err(LoadError::Unsupported);
    }
    if u32(bytes, optional + 52)? != 0 || u32(bytes, optional + 88)? != 0 {
        return Err(LoadError::Malformed);
    }
    let section_alignment = u32(bytes, optional + 32)?;
    let file_alignment = u32(bytes, optional + 36)?;
    if !section_alignment.is_power_of_two()
        || !file_alignment.is_power_of_two()
        || section_alignment < file_alignment
        || (section_alignment < 4096 && file_alignment != section_alignment)
        || (section_alignment >= 4096 && !(512..=65536).contains(&file_alignment))
    {
        return Err(LoadError::Malformed);
    }
    let preferred_base = u32(bytes, optional + 28)?;
    let image_size = align(
        u64::from(u32(bytes, optional + 56)?),
        section_alignment.max(4096),
    )
    .try_into()
    .map_err(|_| LoadError::Malformed)?;
    let header_size = u32(bytes, optional + 60)?;
    if preferred_base == 0
        || !preferred_base.is_multiple_of(65536)
        || image_size == 0
        || u64::from(preferred_base) + u64::from(image_size) > 1 << 32
        || header_size == 0
        || !header_size.is_multiple_of(file_alignment)
        || (header_size as usize) < table_end
        || header_size as usize > bytes.len()
        || align(u64::from(header_size), section_alignment) > u64::from(image_size)
    {
        return Err(LoadError::Malformed);
    }
    let directory_count = u32(bytes, optional + 92)? as usize;
    if directory_count > (optional_size - 96) / 8 {
        return Err(LoadError::Malformed);
    }
    let mut directories = [None; 16];
    let mut certificate = None;
    let directory_bytes = &bytes[optional + 96..optional + 96 + directory_count * 8];
    for (index, record) in directory_bytes.chunks_exact(8).enumerate() {
        let rva = u32(record, 0)?;
        let size = u32(record, 4)?;
        if rva == 0 && size == 0 {
            continue;
        }
        if index >= 16 || matches!(index, 7 | 8 | 14 | 15) {
            return Err(LoadError::Unsupported);
        }
        if rva == 0 || size == 0 {
            return Err(LoadError::Malformed);
        }
        if index == 4 {
            let end = u64::from(rva) + u64::from(size);
            if !rva.is_multiple_of(8) || end > bytes.len() as u64 {
                return Err(LoadError::Malformed);
            }
            certificate = Some(rva as usize..end as usize);
        } else {
            directories[index] = Some(Directory32 { rva, size });
        }
    }
    let mut sections = Vec::new();
    budget.entries(count)?;
    reserve(&mut sections, count)?;
    let mut previous_end = align(u64::from(header_size), section_alignment);
    let mut raw_ranges = Vec::new();
    reserve(&mut raw_ranges, count)?;
    for index in 0..count {
        let at = table + index * 40;
        let virtual_size = u32(bytes, at + 8)?;
        let rva = u32(bytes, at + 12)?;
        let raw_size = u32(bytes, at + 16)?;
        let raw_start = u32(bytes, at + 20)?;
        let flags = u32(bytes, at + 36)?;
        if bytes[at + 24..at + 36].iter().any(|byte| *byte != 0) {
            return Err(LoadError::Unsupported);
        }
        let extent = u64::from(virtual_size.max(raw_size));
        let end = u64::from(rva) + align(extent, section_alignment);
        if !rva.is_multiple_of(section_alignment)
            || u64::from(rva) < previous_end
            || end > u64::from(image_size)
        {
            return Err(LoadError::Malformed);
        }
        let raw = if raw_size == 0 {
            0..0
        } else {
            let raw_end = u64::from(raw_start) + u64::from(raw_size);
            if !raw_start.is_multiple_of(file_alignment)
                || !raw_size.is_multiple_of(file_alignment)
                || raw_start < header_size
                || raw_end > bytes.len() as u64
                || (section_alignment < 4096 && raw_start != rva)
            {
                return Err(LoadError::Malformed);
            }
            let range = raw_start as usize..raw_end as usize;
            if raw_ranges.iter().any(|previous: &std::ops::Range<usize>| {
                previous.start < range.end && range.start < previous.end
            }) {
                return Err(LoadError::Malformed);
            }
            raw_ranges.push(range.clone());
            range
        };
        let permissions = Permissions::from_bits((((flags >> 30) & 3) | ((flags >> 27) & 4)) as u8)
            .ok_or(LoadError::Malformed)?;
        sections.push(Section32 {
            rva,
            virtual_size,
            raw,
            permissions,
        });
        previous_end = end;
    }
    let entry = u32(bytes, optional + 16)?;
    if let Some(range) = &certificate {
        if range.start < header_size as usize
            || raw_ranges
                .iter()
                .any(|raw| raw.start < range.end && range.start < raw.end)
        {
            return Err(LoadError::Malformed);
        }
        let mut at = range.start;
        while at < range.end {
            budget.entries(1)?;
            if range.end - at < 8 {
                return Err(LoadError::Malformed);
            }
            let size = u32(bytes, at)?;
            if size < 8 {
                return Err(LoadError::Malformed);
            }
            let end = at as u64 + align(u64::from(size), 8);
            if end > range.end as u64 {
                return Err(LoadError::Malformed);
            }
            at = end as usize;
        }
    }
    let kind = if characteristics & 0x2000 != 0 {
        ImageKind32::Dll
    } else {
        ImageKind32::Exe
    };
    let entry_rva = if entry == 0 && kind == ImageKind32::Dll {
        None
    } else {
        if !sections.iter().any(|section| {
            entry >= section.rva
                && u64::from(entry) < u64::from(section.rva) + section.extent()
                && section.permissions.allows(crate::memory::Access::Execute)
        }) {
            return Err(LoadError::Malformed);
        }
        Some(entry)
    };
    let mut spans = Vec::new();
    reserve(&mut spans, count + 1)?;
    spans.push(0..align(u64::from(header_size), 4096));
    spans.extend(
        sections
            .iter()
            .map(Section32::page_span)
            .filter(|span| span.start < span.end),
    );
    spans.sort_unstable_by_key(|span| span.start);
    let (mut mapped_pages, mut end) = (0u64, 0u64);
    for span in spans {
        if span.end > end {
            mapped_pages += (span.end - span.start.max(end)) / 4096;
            end = span.end;
        }
    }
    let image = PreparedPe32 {
        kind,
        preferred_base,
        image_size,
        mapped_pages: mapped_pages as u32,
        entry_rva,
        header_size,
        stack_reserve: u32(bytes, optional + 72)?,
        stack_commit: u32(bytes, optional + 76)?,
        heap_reserve: u32(bytes, optional + 80)?,
        heap_commit: u32(bytes, optional + 84)?,
        dll_characteristics,
        sections,
        imports: Vec::new(),
        exports: Exports32::default(),
        tls: None,
        load_config: None,
        delay_import_directory: directories[13],
        relocation_directory: directories[5],
        directories,
        relocations: Vec::new(),
        relocs_stripped: characteristics & 1 != 0,
        source_length: bytes.len(),
    };
    for directory in image.directories.iter().flatten() {
        image.mapped_span(directory.rva, directory.size)?;
    }
    if image.relocs_stripped && image.relocation_directory.is_some() {
        return Err(LoadError::Malformed);
    }
    if image.stack_commit > image.stack_reserve || image.heap_commit > image.heap_reserve {
        return Err(LoadError::Malformed);
    }
    Ok(image)
}
