use super::{MessageQueue32, SentQueue32};
use crate::windows::user32::User32Error32;

pub(crate) struct PreparedSentChange32 {
    thread: u32,
    previous: Option<SentQueue32>,
    next: Option<SentQueue32>,
    previous_generation: u64,
    status: (u32, u32, u64),
}
impl PreparedSentChange32 {
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
        if thread == 0 || delta == 0 {
            return Err(User32Error32::InvalidRequest);
        }
        let previous = self.sent.iter().find(|sent| sent.thread == thread).copied();
        let count = previous
            .map_or(0, |sent| sent.count)
            .checked_add_signed(delta)
            .ok_or(User32Error32::Capacity)?;
        if previous.is_none() && count != 0 && self.sent.len() == self.limit {
            return Err(User32Error32::Capacity);
        }
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
