mod publication;
mod snapshot;

use crate::windows::automation::{
    ArrayLayout32, AutomationError32 as E, AutomationLimits32, variants::Variant32,
};

#[derive(Clone, Copy)]
enum Place32 {
    Root(usize),
    Block { index: usize, offset: usize },
}

struct Fixup32 {
    place: Place32,
    block: usize,
    addend: u32,
    original: [u8; 4],
}

struct ArrayCopy32 {
    source: u32,
    block: usize,
    layout: ArrayLayout32,
}
struct RetiredArray32 {
    pointer: u32,
    vartype: u16,
    bytes: Vec<u8>,
}

pub(in crate::process::windows::automation) struct OwnedValuePlan32 {
    root: Vec<u8>,
    blocks: Vec<Vec<u8>>,
    fixups: Vec<Fixup32>,
    strings: Vec<usize>,
    arrays: Vec<ArrayCopy32>,
    retire: Vec<u32>,
    retire_identities: Vec<(u32, u64)>,
    retired_strings: usize,
    retired_arrays: usize,
    freed: usize,
    bytes: usize,
    source: Variant32,
    old: Variant32,
    retired_payloads: Vec<RetiredArray32>,
    retired_payload_bytes: usize,
}

impl OwnedValuePlan32 {
    fn new(root: &[u8]) -> Result<Self, E> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(root.len())
            .map_err(|_| E::Allocation)?;
        bytes.extend_from_slice(root);
        Ok(Self {
            root: bytes,
            blocks: Vec::new(),
            fixups: Vec::new(),
            strings: Vec::new(),
            arrays: Vec::new(),
            retire: Vec::new(),
            retire_identities: Vec::new(),
            retired_strings: 0,
            retired_arrays: 0,
            freed: 0,
            bytes: 0,
            source: Variant32(root.try_into().map_err(|_| E::InvalidParameter)?),
            old: Variant32::EMPTY,
            retired_payloads: Vec::new(),
            retired_payload_bytes: 0,
        })
    }

    pub(in crate::process::windows::automation) fn source(&self) -> Variant32 {
        self.source
    }
    pub(in crate::process::windows::automation) fn old(&self) -> Variant32 {
        self.old
    }
    pub(in crate::process::windows::automation) fn acquired_root(&mut self, value: Variant32) {
        self.root.copy_from_slice(&value.0);
        self.source = Variant32::EMPTY;
    }
    pub(in crate::process::windows::automation) fn source_array(
        &self,
        pointer: u32,
    ) -> Option<(u16, &[u8])> {
        let array = self.arrays.iter().find(|array| array.source == pointer)?;
        let start = array.layout.data_offset();
        Some((
            array.layout.vartype,
            &self.blocks[array.block][start..start + array.layout.data_bytes],
        ))
    }
    pub(in crate::process::windows::automation) fn old_array(
        &self,
        pointer: u32,
    ) -> Option<(u16, &[u8])> {
        self.retired_payloads
            .iter()
            .find(|array| array.pointer == pointer)
            .map(|array| (array.vartype, array.bytes.as_slice()))
    }

    pub(in crate::process::windows::automation) fn retirement_allocations(&self) -> &[(u32, u64)] {
        &self.retire_identities
    }
    pub(in crate::process::windows::automation) fn absorb_retirement(
        &mut self,
        other: &Self,
        limits: AutomationLimits32,
    ) -> Result<(), E> {
        if other
            .retire
            .iter()
            .any(|pointer| self.retire.binary_search(pointer).is_ok())
        {
            return Err(E::InvalidParameter);
        }
        let count = self
            .retire
            .len()
            .checked_add(other.retire.len())
            .filter(|&count| count <= limits.objects.saturating_mul(2))
            .ok_or(E::Capacity)?;
        let freed = self.freed.checked_add(other.freed).ok_or(E::Capacity)?;
        let strings = self
            .retired_strings
            .checked_add(other.retired_strings)
            .ok_or(E::Capacity)?;
        let arrays = self
            .retired_arrays
            .checked_add(other.retired_arrays)
            .ok_or(E::Capacity)?;
        self.retire
            .try_reserve(count - self.retire.len())
            .map_err(|_| E::Allocation)?;
        self.retire_identities
            .try_reserve(other.retire_identities.len())
            .map_err(|_| E::Allocation)?;
        self.retire.extend_from_slice(&other.retire);
        self.retire.sort_unstable();
        self.retire_identities
            .extend_from_slice(&other.retire_identities);
        self.retire_identities.sort_unstable_by_key(|row| row.0);
        self.freed = freed;
        self.retired_strings = strings;
        self.retired_arrays = arrays;
        Ok(())
    }

    fn reserve_block(&mut self, size: usize, limits: AutomationLimits32) -> Result<usize, E> {
        if self.blocks.len() >= limits.objects {
            return Err(E::Capacity);
        }
        self.bytes = self
            .bytes
            .checked_add(size)
            .filter(|&n| n <= limits.bytes)
            .ok_or(E::Capacity)?;
        self.blocks.try_reserve(1).map_err(|_| E::Allocation)?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(size).map_err(|_| E::Allocation)?;
        bytes.resize(size, 0);
        let index = self.blocks.len();
        self.blocks.push(bytes);
        Ok(index)
    }

    fn fixup(&mut self, place: Place32, block: usize, addend: u32) -> Result<(), E> {
        self.fixups.try_reserve(1).map_err(|_| E::Allocation)?;
        let (bytes, offset) = match place {
            Place32::Root(offset) => (&self.root, offset),
            Place32::Block { index, offset } => (&self.blocks[index], offset),
        };
        let original = bytes[offset..offset + 4].try_into().unwrap();
        self.fixups.push(Fixup32 {
            place,
            block,
            addend,
            original,
        });
        Ok(())
    }

    fn retire_block(
        &mut self,
        pointer: u32,
        bytes: usize,
        limits: AutomationLimits32,
    ) -> Result<(), E> {
        if self.retire.contains(&pointer) {
            return Err(E::InvalidParameter);
        }
        if self.retire.len() >= limits.objects.saturating_mul(2) {
            return Err(E::Capacity);
        }
        self.retire.try_reserve(1).map_err(|_| E::Allocation)?;
        self.retire.push(pointer);
        self.freed = self.freed.checked_add(bytes).ok_or(E::Capacity)?;
        Ok(())
    }
}
