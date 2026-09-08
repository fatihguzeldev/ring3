use super::optional::read_u32;
use super::rva::PreparedPe;
use super::{PeDirectoryAddress, PeRvaError};
use crate::{FileOffset, RelativeVirtualAddress};

const BLOCK_LIMIT: u16 = 256;
const ENTRY_SLOT_LIMIT: u32 = 65_536;

/// raw block metadata; two-byte slots are not individual semantic relocations.
/// page rvas, padding, unknown types and highadj payload words remain unclassified.
///
/// ```compile_fail
/// use ring3_core::{PeBaseRelocationBlock, parse_pe_base_relocation_blocks};
///
/// fn escape() -> Vec<PeBaseRelocationBlock<'static>> {
///     let bytes = vec![0; 64];
///     parse_pe_base_relocation_blocks(&bytes).unwrap()
/// }
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeBaseRelocationBlock<'a> {
    pub block_rva: RelativeVirtualAddress,
    pub block_file_offset: FileOffset,
    pub page_rva: RelativeVirtualAddress,
    pub block_size: u32,
    /// exact even-length bytes after the header, borrowed from the same input.
    pub raw_entries: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeBaseRelocationError {
    Base(PeRvaError),
    InconsistentDirectory {
        rva: RelativeVirtualAddress,
        size: u32,
    },
    DirectoryRangeOverflow {
        rva: RelativeVirtualAddress,
        size: u32,
    },
    BlockLimitExceeded {
        block_index: u16,
        limit: u16,
    },
    UnalignedBlock {
        block_index: u16,
        block_rva: RelativeVirtualAddress,
    },
    TruncatedBlockHeader {
        block_index: u16,
        remaining: u32,
    },
    BlockRange {
        block_index: u16,
        start: RelativeVirtualAddress,
        length: u32,
        cause: PeRvaError,
    },
    InvalidBlockSize {
        block_index: u16,
        block_size: u32,
    },
    BlockExceedsDirectory {
        block_index: u16,
        block_size: u32,
        remaining: u32,
    },
    EntrySlotLimitExceeded {
        block_index: u16,
        total_slots: u32,
        block_slots: u32,
        limit: u32,
    },
}

/// reads at most 256 blocks and 65,536 raw two-byte slots without applying them.
/// each consumed directory prefix must be one conservative file-backed range.
/// missing/zero-zero slots are empty; exact directory size ends the table, not a
/// sentinel. block starts are four-byte aligned; a final size of ten is allowed.
///
/// # errors
/// validates the base file, slot consistency and coordinate end first. per block:
/// block budget, alignment, header size/backing, block size, declared remaining,
/// slot budget, then full-block backing. failures never return a partial list.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_base_relocation_blocks(
    bytes: &[u8],
) -> Result<Vec<PeBaseRelocationBlock<'_>>, PeBaseRelocationError> {
    let prepared = PreparedPe::new(bytes).map_err(PeBaseRelocationError::Base)?;
    let Some(directory) = prepared.headers().directories[5] else {
        return Ok(Vec::new());
    };
    let PeDirectoryAddress::Rva(rva) = directory.address else {
        unreachable!("the same-input parser uses rva coordinates for relocation slot 5");
    };
    let size = directory.size;
    if rva.get() == 0 && size == 0 {
        return Ok(Vec::new());
    }
    if rva.get() == 0 || size == 0 {
        return Err(PeBaseRelocationError::InconsistentDirectory { rva, size });
    }
    if u64::from(rva.get()) + u64::from(size) > 1_u64 << 32 {
        return Err(PeBaseRelocationError::DirectoryRangeOverflow { rva, size });
    }
    let mut blocks = Vec::new();
    let mut consumed = 0;
    let mut total_slots = 0;
    for block_index in 0..=BLOCK_LIMIT {
        if consumed == size {
            return Ok(blocks);
        }
        if block_index == BLOCK_LIMIT {
            return Err(PeBaseRelocationError::BlockLimitExceeded {
                block_index,
                limit: BLOCK_LIMIT,
            });
        }
        let block_rva = RelativeVirtualAddress::new(rva.get() + consumed);
        if !block_rva.get().is_multiple_of(4) {
            return Err(PeBaseRelocationError::UnalignedBlock {
                block_index,
                block_rva,
            });
        }
        let remaining = size - consumed;
        if remaining < 8 {
            return Err(PeBaseRelocationError::TruncatedBlockHeader {
                block_index,
                remaining,
            });
        }
        let resolve = |length| {
            prepared
                .resolve(rva, length)
                .map_err(|cause| PeBaseRelocationError::BlockRange {
                    block_index,
                    start: rva,
                    length,
                    cause,
                })
        };
        let prefix = resolve(consumed + 8)?;
        let header = &prefix.bytes[consumed as usize..];
        let page_rva = RelativeVirtualAddress::new(read_u32(header, 0));
        let block_size = read_u32(header, 4);
        if block_size < 8 || !block_size.is_multiple_of(2) {
            return Err(PeBaseRelocationError::InvalidBlockSize {
                block_index,
                block_size,
            });
        }
        if block_size > remaining {
            return Err(PeBaseRelocationError::BlockExceedsDirectory {
                block_index,
                block_size,
                remaining,
            });
        }
        let block_slots = (block_size - 8) / 2;
        if block_slots > ENTRY_SLOT_LIMIT - total_slots {
            return Err(PeBaseRelocationError::EntrySlotLimitExceeded {
                block_index,
                total_slots,
                block_slots,
                limit: ENTRY_SLOT_LIMIT,
            });
        }
        let end = consumed + block_size;
        let prefix = resolve(end)?;
        blocks.push(PeBaseRelocationBlock {
            block_rva,
            block_file_offset: FileOffset::new(prefix.file_offset.get() + u64::from(consumed)),
            page_rva,
            block_size,
            raw_entries: &prefix.bytes[(consumed + 8) as usize..end as usize],
        });
        total_slots += block_slots;
        consumed = end;
    }
    unreachable!("the final iteration returns completion or the block limit error")
}
