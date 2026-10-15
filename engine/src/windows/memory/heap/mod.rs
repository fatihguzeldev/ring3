mod allocation;
mod lifetime;
mod movable;
mod queries;

use super::*;
use crate::memory::{AddressSpace, GuestAddress, PAGE_SIZE, PageRange};

#[derive(Clone, Copy)]
struct Block32 {
    address: u32,
    extent: u32,
    size: Option<u32>,
}
struct Heap32 {
    handle: u32,
    maximum: u32,
    flags: u32,
    blocks: Vec<Block32>,
    locks: u32,
}
struct Movable32 {
    handle: u32,
    pointer: u32,
    locks: u16,
    discardable: bool,
}
pub(crate) struct HeapManager32 {
    heaps: Vec<Heap32>,
    movable: Vec<Movable32>,
    default: Option<u32>,
    limits: MemoryLimits32,
    next_movable: u32,
}
impl HeapManager32 {
    pub(crate) fn new(limits: MemoryLimits32) -> Result<Self, MemoryError32> {
        if limits.heaps == 0
            || limits.heaps > 4096
            || limits.blocks == 0
            || limits.blocks > 1_048_576
            || limits.moveable_handles == 0
            || limits.moveable_handles > 65536
            || limits.heap_reserve_bytes < 65536
            || !limits.heap_reserve_bytes.is_multiple_of(65536)
            || limits.heap_reserve_bytes > 0x6000_0000
        {
            return Err(MemoryError32::InvalidLimits);
        }
        Ok(Self {
            heaps: Vec::new(),
            movable: Vec::new(),
            default: None,
            limits,
            next_movable: 0x0100_0002,
        })
    }
    pub(crate) fn default_heap(&self) -> Option<u32> {
        self.default
    }
    pub(crate) fn handles(&self) -> impl Iterator<Item = u32> + '_ {
        self.heaps.iter().map(|heap| heap.handle)
    }
    fn index(&self, handle: u32) -> Result<usize, MemoryError32> {
        self.heaps
            .iter()
            .position(|heap| heap.handle == handle)
            .ok_or(MemoryError32::InvalidHandle)
    }
    fn block(&self, heap: usize, pointer: u32) -> Result<usize, MemoryError32> {
        self.heaps[heap]
            .blocks
            .binary_search_by_key(&pointer, |block| block.address)
            .ok()
            .filter(|&index| self.heaps[heap].blocks[index].size.is_some())
            .ok_or(MemoryError32::InvalidParameter)
    }
    fn reserve_block(&mut self, heap: usize) -> Result<(), MemoryError32> {
        if self
            .heaps
            .iter()
            .map(|heap| heap.blocks.len())
            .sum::<usize>()
            >= self.limits.blocks
        {
            return Err(MemoryError32::Capacity);
        }
        self.heaps[heap]
            .blocks
            .try_reserve(1)
            .map_err(|_| MemoryError32::Allocation)
    }
    fn coalesce(&mut self, heap: usize) {
        let blocks = &mut self.heaps[heap].blocks;
        let mut count = 0;
        for index in 0..blocks.len() {
            let block = blocks[index];
            if count != 0 && blocks[count - 1].size.is_none() && block.size.is_none() {
                blocks[count - 1].extent += block.extent;
            } else {
                blocks[count] = block;
                count += 1;
            }
        }
        blocks.truncate(count);
    }
    pub(crate) fn revoke(&mut self, range: PageRange) {
        self.heaps.retain(|heap| {
            !super::regions::overlap(
                PageRange {
                    first: heap.handle as usize / PAGE_SIZE as usize,
                    count: (heap
                        .blocks
                        .iter()
                        .map(|block| u64::from(block.extent))
                        .sum::<u64>()
                        / u64::from(PAGE_SIZE)) as usize,
                },
                range,
            )
        });
        if self
            .default
            .is_some_and(|handle| !self.heaps.iter().any(|heap| heap.handle == handle))
        {
            self.default = None;
        }
        self.movable.retain(|block| {
            block.pointer == 0
                || self.heaps.iter().any(|heap| {
                    heap.blocks
                        .iter()
                        .any(|item| item.address == block.pointer && item.size.is_some())
                })
        });
    }
}
fn checked_size(size: u32) -> Result<u32, MemoryError32> {
    size.max(1)
        .checked_add(7)
        .map(|size| size / 8 * 8)
        .ok_or(MemoryError32::Capacity)
}
fn buffer(size: usize) -> Result<Vec<u8>, MemoryError32> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(size)
        .map_err(|_| MemoryError32::Allocation)?;
    bytes.resize(size, 0);
    Ok(bytes)
}
