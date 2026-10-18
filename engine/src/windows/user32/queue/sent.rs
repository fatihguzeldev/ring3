use super::{MessageQueue32, SentQueue32};
use crate::windows::user32::User32Error32;

pub(crate) struct PreparedSentChange32 {
    thread: u32,
    previous: Option<SentQueue32>,
    next: Option<SentQueue32>,
    previous_generation: u64,
    status: (u32, u32, u64),
}

pub(crate) struct PreparedSentBatch32 {
    changes: [Option<PreparedSentChange32>; 2],
    previous_generation: u64,
    generation: u64,
}

impl PreparedSentBatch32 {
    pub(crate) fn rows(&self) -> impl Iterator<Item = (u32, u32, u32, u64)> + '_ {
        self.changes.iter().flatten().map(|change| {
            (
                change.thread,
                change.status.0,
                change.status.1,
                self.generation,
            )
        })
    }

    pub(crate) fn queue_status(&self, thread: u32) -> Option<(u32, u32, u64)> {
        self.rows()
            .find(|row| row.0 == thread)
            .map(|(_, current, fresh, generation)| (current, fresh, generation))
    }

    pub(in crate::windows::user32) fn add_flags(&mut self, thread: u32, current: u32, fresh: u32) {
        if let Some(change) = self
            .changes
            .iter_mut()
            .flatten()
            .find(|change| change.thread == thread)
        {
            change.status.0 |= current;
            change.status.1 |= fresh;
        }
    }
}

impl PreparedSentChange32 {
    pub(crate) fn with_flags(mut self, current: u32, fresh: u32) -> Self {
        self.status.0 |= current;
        self.status.1 |= fresh;
        self
    }
    pub(crate) fn thread(&self) -> u32 {
        self.thread
    }
    pub(crate) fn queue_status(&self) -> (u32, u32, u64) {
        self.status
    }
}
impl MessageQueue32 {
    pub(super) fn sent_status(&self, thread: u32) -> (u32, u32) {
        match self.sent.iter().find(|sent| sent.thread == thread) {
            Some(sent) => (0x40, if sent.fresh { 0x40 } else { 0 }),
            None => (0, 0),
        }
    }
    pub(crate) fn prepare_sent_change(
        &self,
        thread: u32,
        delta: i32,
    ) -> Result<PreparedSentChange32, User32Error32> {
        let change = self.prepare_sent_update(thread, delta)?;
        if change.previous.is_none() && self.sent.len() == self.limit {
            return Err(User32Error32::Capacity);
        }
        Ok(change)
    }

    fn prepare_sent_update(
        &self,
        thread: u32,
        delta: i32,
    ) -> Result<PreparedSentChange32, User32Error32> {
        if thread == 0 || delta == 0 {
            return Err(User32Error32::InvalidRequest);
        }
        let previous = self.sent.iter().find(|sent| sent.thread == thread).copied();
        let count = previous
            .map_or(0, |sent| sent.count)
            .checked_add_signed(delta)
            .ok_or(User32Error32::Capacity)?;
        let generation = self
            .generation
            .checked_add(1)
            .ok_or(User32Error32::Capacity)?;
        let next = (count != 0).then_some(SentQueue32 {
            thread,
            count,
            fresh: delta > 0 || previous.is_some_and(|sent| sent.fresh),
        });
        let (current, fresh, _) = self.status(thread);
        let status = (
            (current & !0x40) | if next.is_some() { 0x40 } else { 0 },
            (fresh & !0x40)
                | if next.is_some_and(|sent| sent.fresh) {
                    0x40
                } else {
                    0
                },
            generation,
        );
        Ok(PreparedSentChange32 {
            thread,
            previous,
            next,
            previous_generation: self.generation,
            status,
        })
    }

    pub(crate) fn prepare_sent_batch(
        &self,
        changes: &[(u32, i32)],
    ) -> Result<PreparedSentBatch32, User32Error32> {
        if changes.len() > 2 || changes.len() == 2 && changes[0].0 == changes[1].0 {
            return Err(User32Error32::InvalidRequest);
        }
        let generation = self
            .generation
            .checked_add(changes.len() as u64)
            .ok_or(User32Error32::Capacity)?;
        let mut prepared = [None, None];
        let mut occupied = self.sent.len();
        for (index, &(thread, delta)) in changes.iter().enumerate() {
            let change = self.prepare_sent_update(thread, delta)?;
            occupied = occupied + usize::from(change.previous.is_none())
                - usize::from(change.next.is_none());
            prepared[index] = Some(change);
        }
        if occupied > self.limit {
            return Err(User32Error32::Capacity);
        }
        Ok(PreparedSentBatch32 {
            changes: prepared,
            previous_generation: self.generation,
            generation,
        })
    }

    pub(crate) fn commit_sent_batch(&mut self, prepared: PreparedSentBatch32) {
        assert_eq!(self.generation, prepared.previous_generation);
        // validate every row before publication, including observation-only freshness changes.
        for change in prepared.changes.iter().flatten() {
            assert_eq!(
                self.sent
                    .iter()
                    .find(|row| row.thread == change.thread)
                    .copied(),
                change.previous
            );
        }
        // release rows before inserting so a full queue can transfer ownership atomically.
        for change in prepared
            .changes
            .iter()
            .flatten()
            .filter(|change| change.next.is_none())
        {
            let index = self
                .sent
                .iter()
                .position(|row| row.thread == change.thread)
                .unwrap();
            self.sent.remove(index);
        }
        for next in prepared
            .changes
            .iter()
            .flatten()
            .filter_map(|change| change.next)
        {
            if let Some(index) = self.sent.iter().position(|row| row.thread == next.thread) {
                self.sent[index] = next;
            } else {
                assert!(self.sent.len() < self.sent.capacity());
                self.sent.push(next);
            }
        }
        self.generation = prepared.generation;
    }

    pub(crate) fn commit_sent_change(&mut self, prepared: PreparedSentChange32) {
        assert_eq!(self.generation, prepared.previous_generation);
        let index = self
            .sent
            .iter()
            .position(|sent| sent.thread == prepared.thread);
        assert_eq!(index.map(|index| self.sent[index]), prepared.previous);
        match (index, prepared.next) {
            (Some(index), Some(next)) => self.sent[index] = next,
            (Some(index), None) => {
                self.sent.remove(index);
            }
            (None, Some(next)) => {
                assert!(self.sent.len() < self.sent.capacity());
                self.sent.push(next);
            }
            (None, None) => unreachable!("zero-count send removal was rejected before commit"),
        }
        self.generation = prepared.status.2;
    }
}

#[cfg(test)]
mod tests;
