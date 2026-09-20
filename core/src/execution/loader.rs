use super::{GuestMemory, MemoryError, PAGE_SIZE, Permissions};
use crate::{
    PeDirectoryAddress, PeHeaderError, PeImportLookupError, PeImportSymbol, PeKind, PeSectionTable,
    parse_pe_sections,
};

mod imports;

pub struct LoadedPe32 {
    pub memory: GuestMemory,
    pub image_base: u32,
    pub entry_point: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LoadError {
    Header(PeHeaderError),
    UnsupportedImage,
    UnsupportedDirectory { index: u8 },
    InvalidLayout,
    Imports(PeImportLookupError),
    InvalidImportAddressTable,
    BoundImportsUnsupported,
    UnresolvedImport { module: String, symbol: String },
    Memory(MemoryError),
}

impl From<MemoryError> for LoadError {
    fn from(error: MemoryError) -> Self {
        Self::Memory(error)
    }
}

/// loads an import-free i386 executable at its preferred base into fresh memory.
///
/// # errors
/// rejects unsupported initialization requirements, ambiguous section pages,
/// invalid image geometry, non-executable entry points, and memory-limit failures.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn load_pe32(bytes: &[u8], page_limit: u32) -> Result<LoadedPe32, LoadError> {
    load_image(bytes, page_limit, false, |_, _| None)
}

/// loads a pe32 image and resolves static imports before final section protection.
/// the resolver returns guest addresses, never host pointers. callbacks may have
/// occurred before a later load failure; partially loaded memory never escapes.
///
/// # errors
/// rejects unresolved, malformed, bound or zero-oft imports and invalid iat ranges,
/// along with the image and memory errors of [`load_pe32`].
#[expect(clippy::missing_errors_doc, reason = "project headings are lower case")]
pub fn load_pe32_with_imports(
    bytes: &[u8],
    page_limit: u32,
    resolver: impl FnMut(&str, PeImportSymbol<'_>) -> Option<u32>,
) -> Result<LoadedPe32, LoadError> {
    load_image(bytes, page_limit, true, resolver)
}

fn load_image(
    bytes: &[u8],
    page_limit: u32,
    allow_imports: bool,
    resolver: impl FnMut(&str, PeImportSymbol<'_>) -> Option<u32>,
) -> Result<LoadedPe32, LoadError> {
    let table = parse_pe_sections(bytes).map_err(LoadError::Header)?;
    validate_image(&table, bytes.len(), allow_imports)?;
    let optional = &table.headers.optional;
    let base = optional.image_base;
    let length = u64::from(optional.size_of_image);
    let mut memory = GuestMemory::new(page_limit);
    memory.map_zeroed(base, length, Permissions::READ_WRITE)?;
    let headers_size =
        usize::try_from(optional.size_of_headers).map_err(|_| LoadError::InvalidLayout)?;
    memory.write(base, &bytes[..headers_size])?;
    for section in &table.sections {
        memory.write(
            base + u64::from(section.virtual_address.get()),
            section.raw_data,
        )?;
    }
    if allow_imports {
        imports::bind(bytes, base, &mut memory, resolver)?;
    }
    memory.protect(base, length, Permissions::NONE)?;
    memory.protect(
        base,
        round_pages(u64::from(optional.size_of_headers)),
        Permissions::READ,
    )?;
    for section in &table.sections {
        let size = section.virtual_size.max(section.size_of_raw_data);
        if size == 0 {
            continue;
        }
        let flags = section.characteristics;
        memory.protect(
            base + u64::from(section.virtual_address.get()),
            round_pages(u64::from(size)),
            Permissions {
                read: flags & 0x4000_0000 != 0,
                write: flags & 0x8000_0000 != 0,
                execute: flags & 0x2000_0000 != 0,
            },
        )?;
    }
    let entry = base + u64::from(optional.address_of_entry_point.get());
    memory.fetch(entry, &mut [0])?;
    Ok(LoadedPe32 {
        memory,
        image_base: u32::try_from(base).map_err(|_| LoadError::InvalidLayout)?,
        entry_point: u32::try_from(entry).map_err(|_| LoadError::InvalidLayout)?,
    })
}

fn validate_image(
    table: &PeSectionTable<'_>,
    file_size: usize,
    allow_imports: bool,
) -> Result<(), LoadError> {
    let header = &table.headers;
    let optional = &header.optional;
    if header.prefix.kind != PeKind::Pe32
        || header.prefix.machine != 0x14c
        || header.prefix.characteristics & 0x2002 != 2
        || !matches!(optional.subsystem, 2 | 3)
    {
        return Err(LoadError::UnsupportedImage);
    }
    for index in [1_u8, 9, 10, 11, 12, 13, 14] {
        if (allow_imports && matches!(index, 1 | 12)) || (!allow_imports && index == 11) {
            continue;
        }
        if let Some(directory) = header.directories[usize::from(index)] {
            let address = match directory.address {
                PeDirectoryAddress::Rva(rva) => u64::from(rva.get()),
                PeDirectoryAddress::FileOffset(offset) => offset.get(),
            };
            if address != 0 || directory.size != 0 {
                return Err(LoadError::UnsupportedDirectory { index });
            }
        }
    }
    let header_end = header.prefix.pe_offset.get()
        + 24
        + u64::from(header.prefix.size_of_optional_header)
        + u64::from(header.prefix.number_of_sections) * 40;
    let image_size = u64::from(optional.size_of_image);
    if !optional.section_alignment.is_power_of_two()
        || optional.section_alignment < 4096
        || !optional.file_alignment.is_power_of_two()
        || !(512..=65536).contains(&optional.file_alignment)
        || optional.file_alignment > optional.section_alignment
        || !optional.image_base.is_multiple_of(65536)
        || image_size == 0
        || !optional
            .size_of_image
            .is_multiple_of(optional.section_alignment)
        || optional
            .image_base
            .checked_add(image_size)
            .is_none_or(|end| end > 1_u64 << 32)
        || u64::from(optional.size_of_headers) < header_end
        || u64::from(optional.size_of_headers) > file_size as u64
        || u64::from(optional.size_of_headers) > image_size
        || !optional
            .size_of_headers
            .is_multiple_of(optional.file_alignment)
        || u64::from(optional.address_of_entry_point.get()) >= image_size
    {
        return Err(LoadError::InvalidLayout);
    }
    let mut ranges = vec![(0, round_pages(u64::from(optional.size_of_headers)))];
    for section in &table.sections {
        let start = u64::from(section.virtual_address.get());
        let size = section.virtual_size.max(section.size_of_raw_data);
        let end = start + round_pages(u64::from(size));
        if !section
            .virtual_address
            .get()
            .is_multiple_of(optional.section_alignment)
            || end > image_size
            || (section.size_of_raw_data != 0
                && (!section
                    .pointer_to_raw_data
                    .get()
                    .is_multiple_of(u64::from(optional.file_alignment))
                    || !section
                        .size_of_raw_data
                        .is_multiple_of(optional.file_alignment)))
        {
            return Err(LoadError::InvalidLayout);
        }
        if size != 0 {
            if ranges
                .iter()
                .any(|&(other_start, other_end)| start < other_end && other_start < end)
            {
                return Err(LoadError::InvalidLayout);
            }
            ranges.push((start, end));
        }
    }
    Ok(())
}

fn round_pages(size: u64) -> u64 {
    size.div_ceil(PAGE_SIZE) * PAGE_SIZE
}
