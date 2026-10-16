use super::super::User32Context32;
use crate::windows::user32::{PreparedSentChange32, User32Error32};

pub(in crate::process) struct PreparedUser32Sent32 {
    change: PreparedSentChange32,
    timers: (u32, u32),
}
impl PreparedUser32Sent32 {
    pub(in crate::process) fn thread(&self) -> u32 {
        self.change.thread()
    }
    pub(in crate::process) fn queue_status(&self) -> (u32, u32, u64) {
        let (current, fresh, generation) = self.change.queue_status();
        (current | self.timers.0, fresh | self.timers.1, generation)
    }
}
impl User32Context32 {
    pub(in crate::process) fn prepare_sent_change(
        &self,
        thread: u32,
        delta: i32,
    ) -> Result<PreparedUser32Sent32, User32Error32> {
        self.state
            .prepare_sent_change(thread, delta)
            .map(|change| PreparedUser32Sent32 {
                change,
                timers: self.timers.status_at(thread, self.timers.now),
            })
    }
    pub(in crate::process) fn commit_sent_change(&mut self, prepared: PreparedUser32Sent32) {
        self.state.commit_sent_change(prepared.change);
    }
}
