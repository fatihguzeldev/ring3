use super::context::CacheEntry32;
use crate::cpu::dbt::RegistryError;
use std::sync::atomic::Ordering;

const ABSENT: usize = usize::MAX;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Candidate {
    current: bool,
    last: u64,
    slot: u32,
    id: u64,
}

pub(super) struct VictimIndex {
    heap: Vec<Candidate>,
    positions: Vec<usize>,
    pinned: Vec<Candidate>,
    version: Option<u64>,
}

impl VictimIndex {
    pub(super) fn new(slots: usize) -> Result<Self, RegistryError> {
        let mut heap = Vec::new();
        let mut positions = Vec::new();
        let mut pinned = Vec::new();
        heap.try_reserve_exact(slots)
            .map_err(|_| RegistryError::Allocation)?;
        positions
            .try_reserve_exact(slots)
            .map_err(|_| RegistryError::Allocation)?;
        pinned
            .try_reserve_exact(slots)
            .map_err(|_| RegistryError::Allocation)?;
        positions.resize(slots, ABSENT);
        Ok(Self {
            heap,
            positions,
            pinned,
            version: None,
        })
    }

    pub(super) fn len(&self) -> usize {
        self.heap.len()
    }

    pub(super) fn install(&mut self, slot: u32, id: u64, last: u64) {
        self.remove(slot);
        self.insert(Candidate {
            current: true,
            last,
            slot,
            id,
        });
    }

    pub(super) fn remove(&mut self, slot: u32) {
        let position = self.positions[slot as usize];
        if position != ABSENT {
            self.remove_at(position);
        }
    }

    pub(super) fn select(
        &mut self,
        entries: &[Option<CacheEntry32>],
        version: u64,
        current: impl Fn(u64) -> bool,
        pins: &[u64],
    ) -> Option<u32> {
        if self.version != Some(version) {
            for candidate in &mut self.heap {
                candidate.current = current(candidate.id);
            }
            for index in (0..self.heap.len() / 2).rev() {
                self.down(index);
            }
            self.version = Some(version);
        }
        let selected = loop {
            let Some(candidate) = self.heap.first().copied() else {
                break None;
            };
            let row = entries[candidate.slot as usize]
                .as_ref()
                .expect("victim index retains installed entries");
            debug_assert_eq!(row.id, candidate.id);
            let last = row.last.load(Ordering::Relaxed);
            if candidate.last != last {
                // touch timestamps only increase; refresh lazily without work on the hit path.
                self.heap[0].last = last;
                self.down(0);
                continue;
            }
            if pins.binary_search(&candidate.id).is_ok() {
                let pinned = self.remove_at(0);
                self.pinned.push(pinned);
                continue;
            }
            break Some(candidate.slot);
        };
        while let Some(candidate) = self.pinned.pop() {
            self.insert(candidate);
        }
        selected
    }

    fn insert(&mut self, candidate: Candidate) {
        let index = self.heap.len();
        self.positions[candidate.slot as usize] = index;
        self.heap.push(candidate);
        self.up(index);
    }

    fn remove_at(&mut self, index: usize) -> Candidate {
        let removed = self.heap.swap_remove(index);
        self.positions[removed.slot as usize] = ABSENT;
        if let Some(replacement) = self.heap.get(index) {
            self.positions[replacement.slot as usize] = index;
            if index > 0 && self.heap[index] < self.heap[(index - 1) / 2] {
                self.up(index);
            } else {
                self.down(index);
            }
        }
        removed
    }

    fn swap(&mut self, a: usize, b: usize) {
        self.heap.swap(a, b);
        self.positions[self.heap[a].slot as usize] = a;
        self.positions[self.heap[b].slot as usize] = b;
    }

    fn up(&mut self, mut index: usize) {
        while index > 0 {
            let parent = (index - 1) / 2;
            if self.heap[parent] <= self.heap[index] {
                break;
            }
            self.swap(parent, index);
            index = parent;
        }
    }

    fn down(&mut self, mut index: usize) {
        loop {
            let left = index * 2 + 1;
            if left >= self.heap.len() {
                break;
            }
            let right = left + 1;
            let child = if right < self.heap.len() && self.heap[right] < self.heap[left] {
                right
            } else {
                left
            };
            if self.heap[index] <= self.heap[child] {
                break;
            }
            self.swap(index, child);
            index = child;
        }
    }
}
