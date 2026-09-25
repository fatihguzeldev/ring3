use std::collections::VecDeque;

use super::{Access, DispatchError, GuestMemory, guest};

#[derive(Default)]
pub(super) struct Events {
    entries: VecDeque<[u32; 4]>,
    overflow: bool,
}

impl Events {
    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.overflow = false;
    }

    pub(super) fn push(&mut self, event: [u32; 4], capacity: u32) {
        if capacity == 0 || self.overflow {
            return;
        }
        if self.entries.len() >= usize::try_from(capacity - 1).expect("bounded capacity") {
            self.overflow = true;
            return;
        }
        self.entries.push_back(event);
    }

    pub(super) fn read(
        &mut self,
        output: u32,
        count: u32,
        peek: bool,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        let mut requested = [0];
        guest::read_words(memory, count, &mut requested)?;
        guest::check(memory, count, 4, Access::Write)?;
        let returned = self
            .entries
            .len()
            .min(usize::try_from(requested[0]).expect("u32 count"));
        if output != 0 && returned != 0 {
            guest::check(memory, output, returned * 16, Access::Write)?;
            let bytes: Vec<_> = self
                .entries
                .iter()
                .take(returned)
                .flat_map(|record| record.iter().flat_map(|word| word.to_le_bytes()))
                .collect();
            memory.write(u64::from(output), &bytes)?;
        }
        guest::write_word(
            memory,
            count,
            u32::try_from(returned).expect("bounded event count"),
        )?;
        let result = u32::from(self.overflow);
        if !peek {
            drop(self.entries.drain(..returned));
            self.overflow = false;
        }
        Ok(result)
    }
}
