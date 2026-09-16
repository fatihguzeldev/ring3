mod names;

pub use names::{
    PeBoundImportName, PeBoundImportNameError, PeBoundImportNameLocation, PeBoundImportNameTable,
    parse_pe_bound_import_names,
};

use crate::pe::optional::{read_u16, read_u32};
use crate::pe::rva::PreparedPe;
use crate::pe::{PeDirectoryAddress, PeKind, PeRvaError};
use crate::{FileOffset, RelativeVirtualAddress};

const DESCRIPTOR_LIMIT: u16 = 128;
const FORWARDER_LIMIT: u16 = 1024;

/// a count-declared raw reference; reserved words and name offsets stay opaque.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeBoundForwarderRef {
    pub reference_rva: RelativeVirtualAddress,
    pub reference_file_offset: FileOffset,
    pub time_date_stamp: u32,
    pub module_name_offset: u16,
    pub reserved: u16,
}

/// raw fields and the immediately following references, with original coordinates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeBoundImportDescriptor {
    pub descriptor_rva: RelativeVirtualAddress,
    pub descriptor_file_offset: FileOffset,
    pub time_date_stamp: u32,
    pub module_name_offset: u16,
    pub number_of_module_forwarder_refs: u16,
    pub forwarder_refs: Vec<PeBoundForwarderRef>,
}

/// an owned present table, including the terminator of an empty table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeBoundImportTable {
    pub kind: PeKind,
    pub directory_rva: RelativeVirtualAddress,
    pub directory_file_offset: FileOffset,
    pub directory_size: u32,
    pub descriptors: Vec<PeBoundImportDescriptor>,
    pub terminator_rva: RelativeVirtualAddress,
    pub terminator_file_offset: FileOffset,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeBoundImportError {
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
    ForwarderLimitExceeded {
        descriptor_index: u16,
        used: u16,
        count: u16,
        limit: u16,
    },
    TruncatedForwarderRefs {
        descriptor_index: u16,
        count: u16,
        remaining: u32,
    },
    ForwarderRange {
        descriptor_index: u16,
        count: u16,
        start: RelativeVirtualAddress,
        length: u32,
        cause: PeRvaError,
    },
}

/// reads at most 128 raw bound-import descriptors and 1024 total references.
/// offsets are relative to the first descriptor but are never dereferenced.
/// timestamps and reserved words remain opaque; binding validity is not inferred.
/// an all-zero descriptor is required by this reader's acceptance policy.
/// a count-declared all-zero reference is data, not a terminator.
/// each consumed prefix must be one conservative file-backed range; the declared
/// tail after the terminator is unread. missing and zero/zero slots are absent.
/// the 9224-byte consumed-prefix bound excludes supplied file size, base parsing,
/// resolver work and allocator overhead. it is not a whole-call memory or time
/// bound. this synchronous function offers no cancellation.
///
/// ```
/// use ring3_core::{PeBoundImportError, parse_pe_bound_import_descriptors};
/// assert!(matches!(
///     parse_pe_bound_import_descriptors(&[]),
///     Err(PeBoundImportError::Base(_))
/// ));
/// ```
///
/// # errors
/// base parsing, slot consistency and coordinate end precede all table reads.
/// per descriptor: remaining bytes, prefix backing, sentinel, descriptor cap.
/// then: aggregate reference cap, group remaining bytes, whole-group backing.
/// errors return no partial table. no name, reserved-word or binding check occurs.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_bound_import_descriptors(
    bytes: &[u8],
) -> Result<Option<PeBoundImportTable>, PeBoundImportError> {
    let prepared = PreparedPe::new(bytes).map_err(PeBoundImportError::Base)?;
    parse_prepared_table(&prepared)
}

fn parse_prepared_table(
    prepared: &PreparedPe<'_>,
) -> Result<Option<PeBoundImportTable>, PeBoundImportError> {
    let Some(directory) = prepared.headers().directories[11] else {
        return Ok(None);
    };
    let PeDirectoryAddress::Rva(rva) = directory.address else {
        unreachable!("same-input slot 11 is an rva");
    };
    let size = directory.size;
    if rva.get() == 0 && size == 0 {
        return Ok(None);
    }
    if rva.get() == 0 || size == 0 {
        return Err(PeBoundImportError::InconsistentDirectory { rva, size });
    }
    if u64::from(rva.get()) + u64::from(size) > 1_u64 << 32 {
        return Err(PeBoundImportError::DirectoryRangeOverflow { rva, size });
    }
    parse_table(prepared, rva, size)
}

fn parse_table(
    prepared: &PreparedPe<'_>,
    rva: RelativeVirtualAddress,
    size: u32,
) -> Result<Option<PeBoundImportTable>, PeBoundImportError> {
    let mut descriptors = Vec::new();
    let mut cursor = 0;
    let mut used = 0_u16;
    for descriptor_index in 0..=DESCRIPTOR_LIMIT {
        let remaining = size - cursor;
        if remaining == 0 {
            return Err(PeBoundImportError::MissingTerminator { descriptor_index });
        }
        if remaining < 8 {
            return Err(PeBoundImportError::TruncatedDescriptor {
                descriptor_index,
                remaining,
            });
        }
        let descriptor_end = cursor + 8;
        let prefix = prepared.resolve(rva, descriptor_end).map_err(|cause| {
            PeBoundImportError::DescriptorRange {
                descriptor_index,
                start: rva,
                length: descriptor_end,
                cause,
            }
        })?;
        let record = &prefix.bytes[cursor as usize..];
        let time_date_stamp = read_u32(record, 0);
        let module_name_offset = read_u16(record, 4);
        let count = read_u16(record, 6);
        let descriptor_rva = RelativeVirtualAddress::new(rva.get() + cursor);
        let descriptor_file_offset = FileOffset::new(prefix.file_offset.get() + u64::from(cursor));
        if time_date_stamp == 0 && module_name_offset == 0 && count == 0 {
            return Ok(Some(PeBoundImportTable {
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
            return Err(PeBoundImportError::DescriptorLimitExceeded {
                descriptor_index,
                limit: DESCRIPTOR_LIMIT,
            });
        }
        if u32::from(used) + u32::from(count) > u32::from(FORWARDER_LIMIT) {
            return Err(PeBoundImportError::ForwarderLimitExceeded {
                descriptor_index,
                used,
                count,
                limit: FORWARDER_LIMIT,
            });
        }
        let group_bytes = u32::from(count) * 8;
        let remaining = size - descriptor_end;
        if remaining < group_bytes {
            return Err(PeBoundImportError::TruncatedForwarderRefs {
                descriptor_index,
                count,
                remaining,
            });
        }
        let group_end = descriptor_end + group_bytes;
        let mut forwarder_refs = Vec::new();
        if count != 0 {
            let prefix = prepared.resolve(rva, group_end).map_err(|cause| {
                PeBoundImportError::ForwarderRange {
                    descriptor_index,
                    count,
                    start: rva,
                    length: group_end,
                    cause,
                }
            })?;
            forwarder_refs =
                read_references(prefix.bytes, prefix.file_offset, rva, descriptor_end, count);
        }
        descriptors.push(PeBoundImportDescriptor {
            descriptor_rva,
            descriptor_file_offset,
            time_date_stamp,
            module_name_offset,
            number_of_module_forwarder_refs: count,
            forwarder_refs,
        });
        used += count;
        cursor = group_end;
    }
    unreachable!("the final descriptor terminates or refuses");
}

fn read_references(
    prefix: &[u8],
    file_offset: FileOffset,
    rva: RelativeVirtualAddress,
    start: u32,
    count: u16,
) -> Vec<PeBoundForwarderRef> {
    (0..count)
        .map(|index| {
            let offset = start + u32::from(index) * 8;
            let record = &prefix[offset as usize..];
            PeBoundForwarderRef {
                reference_rva: RelativeVirtualAddress::new(rva.get() + offset),
                reference_file_offset: FileOffset::new(file_offset.get() + u64::from(offset)),
                time_date_stamp: read_u32(record, 0),
                module_name_offset: read_u16(record, 4),
                reserved: read_u16(record, 6),
            }
        })
        .collect()
}
