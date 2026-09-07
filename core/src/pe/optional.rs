use super::{PeHeaderError, PeHeaderPrefix, PeKind, Reader, parse_pe_header_prefix};
use crate::{FileOffset, RelativeVirtualAddress};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeOptionalHeader {
    pub linker_version: (u8, u8),
    pub size_of_code: u32,
    pub size_of_initialized_data: u32,
    pub size_of_uninitialized_data: u32,
    pub address_of_entry_point: RelativeVirtualAddress,
    pub base_of_code: RelativeVirtualAddress,
    pub base_of_data: Option<RelativeVirtualAddress>,
    /// preferred base metadata; does not identify or allocate a guest mapping.
    pub image_base: u64,
    pub section_alignment: u32,
    pub file_alignment: u32,
    pub operating_system_version: (u16, u16),
    pub image_version: (u16, u16),
    pub subsystem_version: (u16, u16),
    pub win32_version_value: u32,
    pub size_of_image: u32,
    pub size_of_headers: u32,
    pub checksum: u32,
    pub subsystem: u16,
    pub dll_characteristics: u16,
    pub size_of_stack_reserve: u64,
    pub size_of_stack_commit: u64,
    pub size_of_heap_reserve: u64,
    pub size_of_heap_commit: u64,
    pub loader_flags: u32,
    pub number_of_rva_and_sizes: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeDirectoryAddress {
    Rva(RelativeVirtualAddress),
    FileOffset(FileOffset),
}

/// a directory descriptor; its target bytes have not been read or validated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeDataDirectory {
    pub address: PeDirectoryAddress,
    pub size: u32,
}

/// decoded metadata, not a validated image or execution eligibility decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeHeaders {
    pub prefix: PeHeaderPrefix,
    pub optional: PeOptionalHeader,
    /// sixteen is a local parser limit, not a format maximum; undeclared slots are absent.
    pub directories: [Option<PeDataDirectory>; 16],
}

pub(super) fn read_u16(bytes: &[u8], index: usize) -> u16 {
    u16::from_le_bytes([bytes[index], bytes[index + 1]])
}

pub(super) fn read_u32(bytes: &[u8], index: usize) -> u32 {
    u32::from_le_bytes([
        bytes[index],
        bytes[index + 1],
        bytes[index + 2],
        bytes[index + 3],
    ])
}

fn read_u64(bytes: &[u8], index: usize) -> u64 {
    u64::from_le_bytes([
        bytes[index],
        bytes[index + 1],
        bytes[index + 2],
        bytes[index + 3],
        bytes[index + 4],
        bytes[index + 5],
        bytes[index + 6],
        bytes[index + 7],
    ])
}

// the caller checks the layout-specific fixed extent before scalar reads.
fn decode_fixed(bytes: &[u8], kind: PeKind) -> PeOptionalHeader {
    let (base_of_data, image_base, reservations, tail) = match kind {
        PeKind::Pe32 => (
            Some(RelativeVirtualAddress::new(read_u32(bytes, 24))),
            u64::from(read_u32(bytes, 28)),
            [72, 76, 80, 84].map(|offset| u64::from(read_u32(bytes, offset))),
            88,
        ),
        PeKind::Pe32Plus => (
            None,
            read_u64(bytes, 24),
            [72, 80, 88, 96].map(|offset| read_u64(bytes, offset)),
            104,
        ),
    };
    PeOptionalHeader {
        linker_version: (bytes[2], bytes[3]),
        size_of_code: read_u32(bytes, 4),
        size_of_initialized_data: read_u32(bytes, 8),
        size_of_uninitialized_data: read_u32(bytes, 12),
        address_of_entry_point: RelativeVirtualAddress::new(read_u32(bytes, 16)),
        base_of_code: RelativeVirtualAddress::new(read_u32(bytes, 20)),
        base_of_data,
        image_base,
        section_alignment: read_u32(bytes, 32),
        file_alignment: read_u32(bytes, 36),
        operating_system_version: (read_u16(bytes, 40), read_u16(bytes, 42)),
        image_version: (read_u16(bytes, 44), read_u16(bytes, 46)),
        subsystem_version: (read_u16(bytes, 48), read_u16(bytes, 50)),
        win32_version_value: read_u32(bytes, 52),
        size_of_image: read_u32(bytes, 56),
        size_of_headers: read_u32(bytes, 60),
        checksum: read_u32(bytes, 64),
        subsystem: read_u16(bytes, 68),
        dll_characteristics: read_u16(bytes, 70),
        size_of_stack_reserve: reservations[0],
        size_of_stack_commit: reservations[1],
        size_of_heap_reserve: reservations[2],
        size_of_heap_commit: reservations[3],
        loader_flags: read_u32(bytes, tail),
        number_of_rva_and_sizes: read_u32(bytes, tail + 4),
    }
}

/// decodes scalar headers and descriptors without reading directory targets.
///
/// # errors
/// returns prefix errors, insufficient declared extents, or a directory count
/// beyond this parser's sixteen-slot scope.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_headers(bytes: &[u8]) -> Result<PeHeaders, PeHeaderError> {
    let prefix = parse_pe_header_prefix(bytes)?;
    let reader = Reader { bytes };
    let (_, coff_offset) = reader.read(prefix.pe_offset, 4)?;
    let (_, optional_offset) = reader.read(coff_offset, 20)?;
    let declared = prefix.size_of_optional_header;
    let fixed_size: usize = match prefix.kind {
        PeKind::Pe32 => 96,
        PeKind::Pe32Plus => 112,
    };
    let extent_error = |required| PeHeaderError::OptionalHeaderExtentTooShort {
        offset: optional_offset,
        required,
        declared,
    };
    if usize::from(declared) < fixed_size {
        return Err(extent_error(fixed_size as u64));
    }
    let (optional_bytes, _) = reader.read(optional_offset, u64::from(declared))?;
    let optional = decode_fixed(optional_bytes, prefix.kind);
    let count = optional.number_of_rva_and_sizes;
    let required = fixed_size as u64 + u64::from(count) * 8;
    if required > u64::from(declared) {
        return Err(extent_error(required));
    }
    if count > 16 {
        let (_, count_offset) = reader.read(optional_offset, (fixed_size - 4) as u64)?;
        return Err(PeHeaderError::DirectoryLimitExceeded {
            offset: count_offset,
            count,
            limit: 16,
        });
    }
    let mut directories = [None; 16];
    for ((index, slot), record) in (0..count)
        .zip(&mut directories)
        .zip(optional_bytes[fixed_size..].chunks_exact(8))
    {
        let raw_address = read_u32(record, 0);
        let address = if index == 4 {
            PeDirectoryAddress::FileOffset(FileOffset::new(u64::from(raw_address)))
        } else {
            PeDirectoryAddress::Rva(RelativeVirtualAddress::new(raw_address))
        };
        *slot = Some(PeDataDirectory {
            address,
            size: read_u32(record, 4),
        });
    }
    Ok(PeHeaders {
        prefix,
        optional,
        directories,
    })
}
