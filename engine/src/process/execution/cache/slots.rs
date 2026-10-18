use crate::cpu::dbt::RegistryError;

pub(super) struct FreeSlots {
    words: Vec<u64>,
    first: usize,
}

impl FreeSlots {
    pub(super) fn new(slots: usize) -> Result<Self, RegistryError> {
        let count = slots.div_ceil(64);
        let mut words = Vec::new();
        words
            .try_reserve_exact(count)
            .map_err(|_| RegistryError::Allocation)?;
        words.resize(count, u64::MAX);
        if !slots.is_multiple_of(64) {
            words[count - 1] = (1u64 << (slots % 64)) - 1;
        }
        Ok(Self { words, first: 0 })
    }

    pub(super) fn first(&self) -> Option<u32> {
        self.words
            .get(self.first)
            .map(|word| (self.first * 64) as u32 + word.trailing_zeros())
    }

    pub(super) fn occupy(&mut self, slot: u32) {
        let word = slot as usize / 64;
        self.words[word] &= !(1 << (slot % 64));
        while self.words.get(self.first) == Some(&0) {
            self.first += 1;
        }
    }

    pub(super) fn release(&mut self, slot: u32) {
        let word = slot as usize / 64;
        self.words[word] |= 1 << (slot % 64);
        self.first = self.first.min(word);
    }
}
