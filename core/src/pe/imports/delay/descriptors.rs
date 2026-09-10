use crate::pe::optional::read_u32;
use crate::pe::rva::PreparedPe;
use crate::pe::{PeDirectoryAddress, PeKind, PeRvaError};
use crate::{FileOffset, RelativeVirtualAddress};

const DESCRIPTOR_LIMIT: u16 = 128;
const DESCRIPTOR_SIZE: u32 = 32;

/// raw wire fields; address words and attributes are neither typed nor validated.
/// the eight words remain u32 in both pe32 and pe32+.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeDelayImportDescriptor {
    pub descriptor_rva: RelativeVirtualAddress,
    pub descriptor_file_offset: FileOffset,
    pub attributes: u32,
    pub dll_name_address: u32,
    pub module_handle_address: u32,
    pub import_address_table_address: u32,
    pub import_name_table_address: u32,
    pub bound_import_address_table_address: u32,
    pub unload_import_address_table_address: u32,
    pub time_date_stamp: u32,
}

/// a present table retains the all-zero terminator even when it has no records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeDelayImportTable {
    pub kind: PeKind,
    pub directory_rva: RelativeVirtualAddress,
    pub directory_file_offset: FileOffset,
    pub directory_size: u32,
    pub descriptors: Vec<PeDelayImportDescriptor>,
    pub terminator_rva: RelativeVirtualAddress,
    pub terminator_file_offset: FileOffset,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeDelayImportError {
    Base(PeRvaError),
    InconsistentDirectory {
        rva: RelativeVirtualAddress,
        size: u32,
    },
    DirectoryRangeOverflow {
        rva: RelativeVirtualAddress,
        size: u32,
    },
    MissingTerminator {
        descriptor_index: u16,
    },
    TruncatedDescriptor {
        descriptor_index: u16,
        remaining: u32,
    },
    DescriptorRange {
        descriptor_index: u16,
        start: RelativeVirtualAddress,
        length: u32,
        cause: PeRvaError,
    },
    DescriptorLimitExceeded {
        descriptor_index: u16,
        limit: u16,
    },
}

/// reads at most 128 raw delay descriptors followed by an all-zero record.
/// each consumed table prefix must be one conservative file-backed range.
/// a missing or zero/zero slot is absent; a present empty table is retained.
/// the declared tail after the terminator and every address target remain unread.
/// no attributes classification, address conversion, alignment rule or binding occurs.
///
/// # errors
/// validates the base file, slot consistency and declared coordinate end first.
/// per record: remaining size, prefix backing, zero terminator, then count limit.
/// failure returns no partial table; the maximum consumed prefix is 4128 bytes.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_delay_import_descriptors(
    bytes: &[u8],
) -> Result<Option<PeDelayImportTable>, PeDelayImportError> {
    let prepared = PreparedPe::new(bytes).map_err(PeDelayImportError::Base)?;
    parse_prepared_descriptors(&prepared)
}

pub(super) fn parse_prepared_descriptors(
    prepared: &PreparedPe<'_>,
) -> Result<Option<PeDelayImportTable>, PeDelayImportError> {
    let Some(directory) = prepared.headers().directories[13] else {
        return Ok(None);
    };
    let PeDirectoryAddress::Rva(rva) = directory.address else {
        unreachable!("the same-input parser uses rva coordinates for delay slot 13");
    };
    let size = directory.size;
    if rva.get() == 0 && size == 0 {
        return Ok(None);
    }
    if rva.get() == 0 || size == 0 {
        return Err(PeDelayImportError::InconsistentDirectory { rva, size });
    }
    if u64::from(rva.get()) + u64::from(size) > 1_u64 << 32 {
        return Err(PeDelayImportError::DirectoryRangeOverflow { rva, size });
    }
    let mut descriptors = Vec::new();
    for descriptor_index in 0..=DESCRIPTOR_LIMIT {
        let offset = u32::from(descriptor_index) * DESCRIPTOR_SIZE;
        let remaining = size - offset;
        if remaining == 0 {
            return Err(PeDelayImportError::MissingTerminator { descriptor_index });
        }
        if remaining < DESCRIPTOR_SIZE {
            return Err(PeDelayImportError::TruncatedDescriptor {
                descriptor_index,
                remaining,
            });
        }
        let length = offset + DESCRIPTOR_SIZE;
        let prefix =
            prepared
                .resolve(rva, length)
                .map_err(|cause| PeDelayImportError::DescriptorRange {
                    descriptor_index,
                    start: rva,
                    length,
                    cause,
                })?;
        let record = &prefix.bytes[offset as usize..];
        let fields = [0, 4, 8, 12, 16, 20, 24, 28].map(|index| read_u32(record, index));
        let descriptor_rva = RelativeVirtualAddress::new(rva.get() + offset);
        let descriptor_file_offset = FileOffset::new(prefix.file_offset.get() + u64::from(offset));
        if fields == [0; 8] {
            return Ok(Some(PeDelayImportTable {
                kind: prepared.headers().prefix.kind,
                directory_rva: rva,
                directory_file_offset: prefix.file_offset,
                directory_size: size,
                descriptors,
                terminator_rva: descriptor_rva,
                terminator_file_offset: descriptor_file_offset,
            }));
        }
        if descriptor_index == DESCRIPTOR_LIMIT {
            return Err(PeDelayImportError::DescriptorLimitExceeded {
                descriptor_index,
                limit: DESCRIPTOR_LIMIT,
            });
        }
        descriptors.push(PeDelayImportDescriptor {
            descriptor_rva,
            descriptor_file_offset,
            attributes: fields[0],
            dll_name_address: fields[1],
            module_handle_address: fields[2],
            import_address_table_address: fields[3],
            import_name_table_address: fields[4],
            bound_import_address_table_address: fields[5],
            unload_import_address_table_address: fields[6],
            time_date_stamp: fields[7],
        });
    }
    unreachable!("the bounded final record either terminates or refuses");
}
