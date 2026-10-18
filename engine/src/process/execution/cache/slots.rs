use crate::cpu::dbt::{RegistryError, RegistryLimits};

pub(super) struct FreeSlots {
    words: Vec<u64>,
    groups: Vec<u64>,
    nonempty: u64,
}

impl FreeSlots {
    pub(super) fn new(slots: usize) -> Result<Self, RegistryError> {
        if !(1..=RegistryLimits::MAX_UNITS).contains(&slots) {
            return Err(RegistryError::InvalidLimits);
        }
        let count = slots.div_ceil(64);
        let mut words = Vec::new();
        words
            .try_reserve_exact(count)
            .map_err(|_| RegistryError::Allocation)?;
        words.resize(count, u64::MAX);
        if !slots.is_multiple_of(64) {
            words[count - 1] = (1u64 << (slots % 64)) - 1;
        }
        let group_count = count.div_ceil(64);
        debug_assert!(group_count <= 64);
        let mut groups = Vec::new();
        groups
            .try_reserve_exact(group_count)
            .map_err(|_| RegistryError::Allocation)?;
        groups.resize(group_count, u64::MAX);
        if !count.is_multiple_of(64) {
            groups[group_count - 1] = (1u64 << (count % 64)) - 1;
        }
        let nonempty = u64::MAX >> (64 - group_count);
        Ok(Self {
            words,
            groups,
            nonempty,
        })
    }

    pub(super) fn first(&self) -> Option<u32> {
        if self.nonempty == 0 {
            return None;
        }
        let group = self.nonempty.trailing_zeros() as usize;
        let word = group * 64 + self.groups[group].trailing_zeros() as usize;
        Some((word * 64) as u32 + self.words[word].trailing_zeros())
    }

    pub(super) fn occupy(&mut self, slot: u32) {
        let word = slot as usize / 64;
        self.words[word] &= !(1 << (slot % 64));
        if self.words[word] == 0 {
            let group = word / 64;
            self.groups[group] &= !(1 << (word % 64));
            if self.groups[group] == 0 {
                self.nonempty &= !(1 << group);
            }
        }
    }

    pub(super) fn release(&mut self, slot: u32) {
        let word = slot as usize / 64;
        self.words[word] |= 1 << (slot % 64);
        self.groups[word / 64] |= 1 << (word % 64);
        self.nonempty |= 1 << (word / 64);
    }
}
