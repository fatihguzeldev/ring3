use super::*;
use crate::memory::{AddressSpace, GuestAddress, PAGE_SIZE, PageRange, PrivatePageChange32};
mod stack;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Committed32 {
    range: PageRange,
    protection: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Region32 {
    range: PageRange,
    protection: u32,
    committed: Vec<Committed32>,
}
pub(crate) struct VirtualMemory32 {
    regions: Vec<Region32>,
    limits: MemoryLimits32,
}
impl VirtualMemory32 {
    pub(crate) fn new(limits: MemoryLimits32) -> Result<Self, MemoryError32> {
        if limits.regions == 0
            || limits.regions > 65536
            || limits.fragments < limits.regions
            || limits.fragments > 1_048_576
        {
            return Err(MemoryError32::InvalidLimits);
        }
        Ok(Self {
            regions: Vec::new(),
            limits,
        })
    }
    pub(crate) fn reservations(&self) -> impl Iterator<Item = PageRange> + '_ {
        self.regions.iter().map(|region| region.range)
    }
    pub(crate) fn reservation_count(&self) -> usize {
        self.regions.len()
    }
    pub(crate) fn committed_ranges(&self) -> impl Iterator<Item = PageRange> + '_ {
        self.regions
            .iter()
            .flat_map(|region| region.committed.iter().map(|segment| segment.range))
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }
    pub(crate) fn revoke(&mut self, range: PageRange) {
        self.regions.retain(|region| !overlap(region.range, range));
    }
    pub(crate) fn allocation_base(&self, address: u32) -> Option<u32> {
        self.region(address)
            .map(|index| start(self.regions[index].range))
    }
    fn region(&self, address: u32) -> Option<usize> {
        let page = address as usize / PAGE_SIZE as usize;
        self.regions
            .iter()
            .position(|region| region.range.indices().contains(&page))
    }
    pub(crate) fn allocate(
        &mut self,
        memory: &mut AddressSpace,
        external: &[PageRange],
        address: u32,
        size: u32,
        flags: u32,
        protect: u32,
    ) -> Result<u32, MemoryError32> {
        self.allocate_with_writes(memory, external, address, size, flags, protect, &[])
    }
    pub(crate) fn allocate_with_writes(
        &mut self,
        memory: &mut AddressSpace,
        external: &[PageRange],
        address: u32,
        size: u32,
        flags: u32,
        protect: u32,
        writes: &[(GuestAddress, &[u8])],
    ) -> Result<u32, MemoryError32> {
        let permissions = protection(protect)?;
        if size == 0 {
            return Err(MemoryError32::InvalidParameter);
        }
        if flags == MEM_RESET || flags == 0x1000000 {
            let range = span(address, size)?;
            let index = self.region(address).ok_or(MemoryError32::InvalidAddress)?;
            if !contains(self.regions[index].range, range)
                || range.indices().any(|page| !self.committed(index, page))
            {
                return Err(MemoryError32::InvalidAddress);
            }
            memory.apply_private_pages(&[], writes)?;
            return Ok(start(range));
        }
        if flags & !(MEM_RESERVE | MEM_COMMIT | MEM_TOP_DOWN) != 0
            || flags & (MEM_RESERVE | MEM_COMMIT) == 0
            || matches!(protect & !PAGE_GUARD, 8 | 0x80)
        {
            return Err(MemoryError32::InvalidParameter);
        }
        let reserve = flags & MEM_RESERVE != 0 || address == 0;
        if reserve {
            if self.regions.len() >= self.limits.regions {
                return Err(MemoryError32::Capacity);
            }
            let range = if address == 0 {
                let count = size.div_ceil(PAGE_SIZE);
                self.choose(memory, external, count, flags & MEM_TOP_DOWN != 0)?
            } else {
                let base = address / 65536 * 65536;
                let length = u64::from(address) + u64::from(size) - u64::from(base);
                let range = span(
                    base,
                    u32::try_from(length).map_err(|_| MemoryError32::InvalidParameter)?,
                )?;
                if base < 65536
                    || self
                        .reservations()
                        .chain(external.iter().copied())
                        .any(|other| overlap(range, other))
                {
                    return Err(MemoryError32::InvalidAddress);
                }
                if memory
                    .find_unmapped_range(range, range.count as u32, PAGE_SIZE)
                    .is_err()
                {
                    return Err(MemoryError32::InvalidAddress);
                }
                range
            };
            self.regions
                .try_reserve(1)
                .map_err(|_| MemoryError32::Allocation)?;
            let mut committed = Vec::new();
            let mut changes = Vec::new();
            if flags & MEM_COMMIT != 0 {
                if range.count
                    > memory
                        .capacity_pages()
                        .saturating_sub(memory.mapped_pages()) as usize
                {
                    return Err(MemoryError32::Capacity);
                }
                committed
                    .try_reserve_exact(1)
                    .map_err(|_| MemoryError32::Allocation)?;
                committed.push(Committed32 {
                    range,
                    protection: protect,
                });
                changes
                    .try_reserve_exact(range.count)
                    .map_err(|_| MemoryError32::Allocation)?;
                for page in range.indices() {
                    changes.push(PrivatePageChange32::Commit {
                        address: GuestAddress(page as u32 * PAGE_SIZE),
                        permissions,
                    });
                }
            }
            self.check_fragments(committed.len(), 0)?;
            memory.apply_private_pages(&changes, writes)?;
            let index = self
                .regions
                .binary_search_by_key(&range.first, |region| region.range.first)
                .unwrap_err();
            self.regions.insert(
                index,
                Region32 {
                    range,
                    protection: protect,
                    committed,
                },
            );
            return Ok(start(range));
        }
        let range = span(address, size)?;
        let index = self.region(address).ok_or(MemoryError32::InvalidAddress)?;
        if !contains(self.regions[index].range, range) {
            return Err(MemoryError32::InvalidAddress);
        }
        let missing = range
            .indices()
            .filter(|&page| !self.committed(index, page))
            .count();
        if missing
            > memory
                .capacity_pages()
                .saturating_sub(memory.mapped_pages()) as usize
        {
            return Err(MemoryError32::Capacity);
        }
        if missing == 0 {
            memory.apply_private_pages(&[], writes)?;
            return Ok(start(range));
        }
        let mut changes = Vec::new();
        changes
            .try_reserve_exact(range.count.min(memory.capacity_pages() as usize))
            .map_err(|_| MemoryError32::Allocation)?;
        let mut committed = self.clone_segments(index, missing)?;
        for page in range.indices() {
            if !self.committed(index, page) {
                changes
                    .try_reserve(1)
                    .map_err(|_| MemoryError32::Allocation)?;
                changes.push(PrivatePageChange32::Commit {
                    address: GuestAddress(page as u32 * PAGE_SIZE),
                    permissions,
                });
                committed.push(Committed32 {
                    range: PageRange {
                        first: page,
                        count: 1,
                    },
                    protection: protect,
                });
            }
        }
        normalize(&mut committed);
        self.check_fragments(committed.len(), self.regions[index].committed.len())?;
        memory.apply_private_pages(&changes, writes)?;
        self.regions[index].committed = committed;
        Ok(start(range))
    }
    pub(crate) fn install_thread_pages(
        &mut self,
        memory: &mut AddressSpace,
        base: u32,
        pages: &[crate::memory::PageInitialization<'_>],
        words: &[crate::memory::WordWrite32],
    ) -> Result<(), MemoryError32> {
        let index = self.region(base).ok_or(MemoryError32::InvalidAddress)?;
        if start(self.regions[index].range) != base || !self.regions[index].committed.is_empty() {
            return Err(MemoryError32::InvalidAddress);
        }
        let mut committed = Vec::new();
        committed
            .try_reserve_exact(pages.len())
            .map_err(|_| MemoryError32::Allocation)?;
        for page in pages {
            let range = PageRange::new(page.address, 1)?;
            if !contains(self.regions[index].range, range) {
                return Err(MemoryError32::InvalidAddress);
            }
            committed.push(Committed32 {
                range,
                protection: protection_value(page.permissions),
            });
        }
        normalize(&mut committed);
        self.check_fragments(committed.len(), 0)?;
        memory.install_pages_with_word_writes(pages, words)?;
        self.regions[index].committed = committed;
        Ok(())
    }
    pub(crate) fn free(
        &mut self,
        memory: &mut AddressSpace,
        address: u32,
        size: u32,
        flags: u32,
    ) -> Result<(), MemoryError32> {
        let index = self.region(address).ok_or(MemoryError32::InvalidAddress)?;
        let region = &self.regions[index];
        let range = match flags {
            MEM_RELEASE if size == 0 && address == start(region.range) => region.range,
            MEM_DECOMMIT if size == 0 && address == start(region.range) => region.range,
            MEM_DECOMMIT if size != 0 => span(address, size)?,
            _ => return Err(MemoryError32::InvalidParameter),
        };
        if !contains(region.range, range) {
            return Err(MemoryError32::InvalidAddress);
        }
        let mut changes = Vec::new();
        let count = region
            .committed
            .iter()
            .map(|segment| intersection(segment.range, range).map_or(0, |range| range.count))
            .sum();
        changes
            .try_reserve_exact(count)
            .map_err(|_| MemoryError32::Allocation)?;
        for segment in &region.committed {
            if let Some(overlap) = intersection(segment.range, range) {
                for page in overlap.indices() {
                    changes.push(PrivatePageChange32::Decommit {
                        address: GuestAddress(page as u32 * PAGE_SIZE),
                    });
                }
            }
        }
        let committed = if flags == MEM_DECOMMIT {
            Some(self.replaced_segments(index, range, None)?)
        } else {
            None
        };
        memory.apply_private_pages(&changes, &[])?;
        if let Some(committed) = committed {
            self.regions[index].committed = committed;
        } else {
            self.regions.remove(index);
        }
        Ok(())
    }
    pub(crate) fn protect(
        &mut self,
        memory: &mut AddressSpace,
        address: u32,
        size: u32,
        protect: u32,
        output: GuestAddress,
    ) -> Result<(), MemoryError32> {
        let range = span(address, size)?;
        let index = self.region(address).ok_or(MemoryError32::InvalidAddress)?;
        if !contains(self.regions[index].range, range)
            || range.indices().any(|page| !self.committed(index, page))
        {
            return Err(MemoryError32::InvalidAddress);
        }
        let permissions = protection(protect)?;
        if matches!(protect & !PAGE_GUARD, 8 | 0x80) {
            return Err(MemoryError32::InvalidParameter);
        }
        let old = protection_value(memory.permissions_at(GuestAddress(start(range)))?);
        let committed = self.replaced_segments(index, range, Some(protect))?;
        let mut changes = Vec::new();
        changes
            .try_reserve_exact(range.count)
            .map_err(|_| MemoryError32::Allocation)?;
        for page in range.indices() {
            changes.push(PrivatePageChange32::Protect {
                address: GuestAddress(page as u32 * PAGE_SIZE),
                permissions,
            });
        }
        memory.apply_private_pages(&changes, &[(output, &old.to_le_bytes())])?;
        self.regions[index].committed = committed;
        Ok(())
    }
    pub(crate) fn query(
        &self,
        memory: &AddressSpace,
        address: u32,
    ) -> Result<MemoryInfo32, MemoryError32> {
        if address >= 0x8000_0000 {
            return Err(MemoryError32::InvalidParameter);
        }
        let base = address / PAGE_SIZE * PAGE_SIZE;
        let page = (base / PAGE_SIZE) as usize;
        if let Some(index) = self.region(address) {
            let region = &self.regions[index];
            let segment = region
                .committed
                .iter()
                .find(|segment| segment.range.indices().contains(&page));
            let mut end = segment.map_or_else(
                || {
                    region
                        .committed
                        .iter()
                        .find(|segment| segment.range.first > page)
                        .map_or(region.range.first + region.range.count, |segment| {
                            segment.range.first
                        })
                },
                |segment| segment.range.first + segment.range.count,
            );
            let current = if segment.is_some() {
                Some(memory.permissions_at(GuestAddress(base))?)
            } else {
                None
            };
            if let Some(current) = current {
                let mut next = page + 1;
                while next < end
                    && memory
                        .permissions_at(GuestAddress(next as u32 * PAGE_SIZE))
                        .ok()
                        == Some(current)
                {
                    next += 1;
                }
                end = next;
            }
            return Ok(MemoryInfo32 {
                base,
                allocation_base: start(region.range),
                allocation_protection: region.protection,
                size: ((end - page) * PAGE_SIZE as usize) as u32,
                state: if segment.is_some() {
                    MEM_COMMIT
                } else {
                    MEM_RESERVE
                },
                protection: current.map_or(0, protection_value),
                kind: MEM_PRIVATE,
            });
        }
        let protection = memory.permissions_at(GuestAddress(base)).ok();
        let mut end = page + 1;
        while end < 0x8000_0000usize / PAGE_SIZE as usize
            && self.region(end as u32 * PAGE_SIZE).is_none()
            && memory
                .permissions_at(GuestAddress(end as u32 * PAGE_SIZE))
                .ok()
                == protection
        {
            end += 1;
        }
        Ok(MemoryInfo32 {
            base,
            allocation_base: if protection.is_some() { base } else { 0 },
            allocation_protection: protection.map_or(0, protection_value),
            size: ((end - page) * PAGE_SIZE as usize) as u32,
            state: if protection.is_some() {
                MEM_COMMIT
            } else {
                MEM_FREE
            },
            protection: protection.map_or(0, protection_value),
            kind: if protection.is_some() { MEM_PRIVATE } else { 0 },
        })
    }
    fn choose(
        &self,
        memory: &AddressSpace,
        external: &[PageRange],
        count: u32,
        top: bool,
    ) -> Result<PageRange, MemoryError32> {
        let end = 0x7000_0000u64;
        if u64::from(count) * u64::from(PAGE_SIZE) > end - 0x1000_0000 {
            return Err(MemoryError32::Capacity);
        }
        let mut lower = 0x1000_0000u64;
        if top {
            let mut candidate = (end - u64::from(count) * u64::from(PAGE_SIZE)) / 65536 * 65536;
            loop {
                let range = PageRange::new(GuestAddress(candidate as u32), count)?;
                if !self
                    .reservations()
                    .chain(external.iter().copied())
                    .any(|other| overlap(range, other))
                    && memory.find_unmapped_range(range, count, PAGE_SIZE).is_ok()
                {
                    return Ok(range);
                }
                if candidate < lower + 65536 {
                    return Err(MemoryError32::Capacity);
                }
                candidate -= 65536;
            }
        }
        loop {
            if lower + u64::from(count) * u64::from(PAGE_SIZE) > end {
                return Err(MemoryError32::Capacity);
            }
            let bounds = PageRange::new(
                GuestAddress(lower as u32),
                ((end - lower) / u64::from(PAGE_SIZE)) as u32,
            )?;
            let range = memory.find_unmapped_range(bounds, count, 65536)?;
            if let Some(next) = self
                .reservations()
                .chain(external.iter().copied())
                .filter(|other| overlap(range, *other))
                .map(|other| (other.first + other.count) as u64 * u64::from(PAGE_SIZE))
                .max()
            {
                lower = next.div_ceil(65536) * 65536;
            } else {
                return Ok(range);
            }
        }
    }
    fn committed(&self, index: usize, page: usize) -> bool {
        self.regions[index]
            .committed
            .iter()
            .any(|segment| segment.range.indices().contains(&page))
    }
    fn clone_segments(
        &self,
        index: usize,
        extra: usize,
    ) -> Result<Vec<Committed32>, MemoryError32> {
        let mut segments = Vec::new();
        segments
            .try_reserve_exact(self.regions[index].committed.len() + extra)
            .map_err(|_| MemoryError32::Allocation)?;
        segments.extend_from_slice(&self.regions[index].committed);
        Ok(segments)
    }
    fn replaced_segments(
        &self,
        index: usize,
        range: PageRange,
        protect: Option<u32>,
    ) -> Result<Vec<Committed32>, MemoryError32> {
        let mut segments = Vec::new();
        segments
            .try_reserve_exact(self.regions[index].committed.len() + 2)
            .map_err(|_| MemoryError32::Allocation)?;
        for segment in &self.regions[index].committed {
            if let Some(overlap) = intersection(segment.range, range) {
                if segment.range.first < overlap.first {
                    segments.push(Committed32 {
                        range: PageRange {
                            first: segment.range.first,
                            count: overlap.first - segment.range.first,
                        },
                        protection: segment.protection,
                    });
                }
                if let Some(protection) = protect {
                    segments.push(Committed32 {
                        range: overlap,
                        protection,
                    });
                }
                let last = overlap.first + overlap.count;
                let end = segment.range.first + segment.range.count;
                if last < end {
                    segments.push(Committed32 {
                        range: PageRange {
                            first: last,
                            count: end - last,
                        },
                        protection: segment.protection,
                    });
                }
            } else {
                segments.push(segment.clone());
            }
        }
        normalize(&mut segments);
        self.check_fragments(segments.len(), self.regions[index].committed.len())?;
        Ok(segments)
    }
    fn check_fragments(&self, add: usize, remove: usize) -> Result<(), MemoryError32> {
        if self
            .regions
            .iter()
            .map(|region| region.committed.len())
            .sum::<usize>()
            - remove
            + add
            > self.limits.fragments
        {
            Err(MemoryError32::Capacity)
        } else {
            Ok(())
        }
    }
}
fn normalize(segments: &mut Vec<Committed32>) {
    segments.sort_unstable_by_key(|segment| segment.range.first);
    let mut count = 0;
    for index in 0..segments.len() {
        let segment = segments[index].clone();
        if count != 0
            && segments[count - 1].range.first + segments[count - 1].range.count
                == segment.range.first
            && segments[count - 1].protection == segment.protection
        {
            segments[count - 1].range.count += segment.range.count;
        } else {
            segments[count] = segment;
            count += 1;
        }
    }
    segments.truncate(count);
}
pub(crate) fn span(address: u32, size: u32) -> Result<PageRange, MemoryError32> {
    let end = u64::from(address) + u64::from(size);
    if size == 0 || end > 1u64 << 32 {
        return Err(MemoryError32::InvalidParameter);
    }
    let base = address / PAGE_SIZE * PAGE_SIZE;
    Ok(PageRange::new(
        GuestAddress(base),
        ((end - u64::from(base)).div_ceil(u64::from(PAGE_SIZE))) as u32,
    )?)
}
pub(crate) fn start(range: PageRange) -> u32 {
    range.first as u32 * PAGE_SIZE
}
pub(crate) fn overlap(a: PageRange, b: PageRange) -> bool {
    a.first < b.first + b.count && b.first < a.first + a.count
}
fn contains(outer: PageRange, inner: PageRange) -> bool {
    inner.first >= outer.first && inner.first + inner.count <= outer.first + outer.count
}
fn intersection(a: PageRange, b: PageRange) -> Option<PageRange> {
    let first = a.first.max(b.first);
    let end = (a.first + a.count).min(b.first + b.count);
    (first < end).then_some(PageRange {
        first,
        count: end.saturating_sub(first),
    })
}
