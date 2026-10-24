use crate::{
    cpu::x86::{
        ControlOperation,
        decode::{
            BoundDecode, CodeDecodeKey, CodeFetchContext, DecodeAuthority, DecodeError,
            DecodePolicy, DecodeScratch, DecodedOperation,
        },
        ir::Operation,
    },
    memory::{AddressSpace, CodeSnapshot, GuestAddress, PageVersion},
};
use std::{mem::size_of, ops::Deref};

const ROWS: usize = 4096;
const WAYS: usize = 4;
const MAX_PAGES: usize = 2;

struct Entry {
    key: CodeDecodeKey,
    operation: DecodedOperation,
    source: CodeSnapshot,
    stamp: u64,
}

pub(super) struct DecodedBook {
    rows: Box<[Option<Entry>]>,
    stamp: u64,
    #[cfg(test)]
    skip_snapshot: bool,
}

// all rows, two consumed pages per row and one pending insertion are reserved
// before a lease. replacement never touches executable residency or its pointers.
pub(crate) const RESERVATION: usize = size_of::<DecodedBook>()
    + ROWS * (size_of::<Option<Entry>>() + MAX_PAGES * size_of::<PageVersion>())
    + size_of::<DecodedOperation>()
    + size_of::<CodeSnapshot>()
    + MAX_PAGES * size_of::<PageVersion>();

pub(super) enum BookStep<'a> {
    Borrowed(&'a DecodedOperation),
    Fresh(DecodedOperation),
}

impl Deref for BookStep<'_> {
    type Target = DecodedOperation;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Borrowed(operation) => operation,
            Self::Fresh(operation) => operation,
        }
    }
}

impl Default for DecodedBook {
    fn default() -> Self {
        Self {
            rows: (0..ROWS).map(|_| None).collect(),
            stamp: 0,
            #[cfg(test)]
            skip_snapshot: false,
        }
    }
}

impl DecodedBook {
    fn first(pc: GuestAddress, key: CodeDecodeKey) -> usize {
        let value = pc.0 ^ key.cs_base.rotate_left(13) ^ u32::from(key.default32);
        let hash = (value ^ (value >> 16)).wrapping_mul(0x7feb_352d);
        (hash as usize & (ROWS / WAYS - 1)) * WAYS
    }

    fn hit(
        entry: &Entry,
        memory: &AddressSpace,
        pc: GuestAddress,
        fetch: CodeFetchContext,
    ) -> bool {
        if entry.operation.pc() != pc
            || entry.key != fetch.decode_key()
            || !memory.is_code_current(&entry.source)
            || !fetch.permits(pc, entry.operation.length())
        {
            return false;
        }
        let mut bytes = [0; 15];
        // failed probes publish nothing. the original progressive decoder owns
        // current first-denied-byte, segment, paging and wrapping fault priority.
        fetch
            .fetch(
                memory,
                pc,
                &mut bytes[..usize::from(entry.operation.length())],
            )
            .is_ok()
    }

    pub(super) fn decode<'a>(
        &'a mut self,
        scratch: &mut DecodeScratch,
        memory: &AddressSpace,
        pc: GuestAddress,
        fetch: CodeFetchContext,
    ) -> Result<BoundDecode<BookStep<'a>>, DecodeError> {
        self.stamp = self.stamp.saturating_add(1);
        let first = Self::first(pc, fetch.decode_key());
        if let Some(index) = (first..first + WAYS).find(|&index| {
            self.rows[index]
                .as_ref()
                .is_some_and(|entry| Self::hit(entry, memory, pc, fetch))
        }) {
            let entry = self.rows[index].as_mut().unwrap();
            entry.stamp = self.stamp;
            return Ok(BoundDecode::Instruction(BookStep::Borrowed(
                &entry.operation,
            )));
        }
        let operation = match scratch.decode(
            memory,
            pc,
            DecodePolicy::Execution,
            Some(fetch),
            DecodeAuthority::default(),
        )? {
            BoundDecode::Instruction(operation) => operation,
            BoundDecode::CodeFault(fault) => return Ok(BoundDecode::CodeFault(fault)),
        };
        if operation.is_privileged_instruction()
            || matches!(
                operation.operation(),
                Operation::Control(ControlOperation::InvalidOpcode)
            )
        {
            return Ok(BoundDecode::Instruction(BookStep::Fresh(operation)));
        }
        #[cfg(test)]
        if self.skip_snapshot {
            return Ok(BoundDecode::Instruction(BookStep::Fresh(operation)));
        }
        let Ok(source) = fetch.snapshot(memory, pc, usize::from(operation.length())) else {
            // optional retention failure must not change successful decoding.
            return Ok(BoundDecode::Instruction(BookStep::Fresh(operation)));
        };
        if source.versions.len() > MAX_PAGES {
            return Ok(BoundDecode::Instruction(BookStep::Fresh(operation)));
        }
        let index = (first..first + WAYS)
            .find(|&index| {
                self.rows[index].as_ref().is_none_or(|entry| {
                    entry.operation.pc() == pc && entry.key == fetch.decode_key()
                })
            })
            .unwrap_or_else(|| {
                (first..first + WAYS)
                    .min_by_key(|&index| self.rows[index].as_ref().unwrap().stamp)
                    .unwrap()
            });
        self.rows[index] = Some(Entry {
            key: fetch.decode_key(),
            operation,
            source,
            stamp: self.stamp,
        });
        Ok(BoundDecode::Instruction(BookStep::Borrowed(
            &self.rows[index].as_ref().unwrap().operation,
        )))
    }
}

#[cfg(test)]
mod tests;
