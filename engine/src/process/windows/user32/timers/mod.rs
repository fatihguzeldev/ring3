mod clock;
pub(in crate::process) use clock::PreparedUser32TimerClock32;
mod delivery;
pub(super) use delivery::TimerMessageQuery32;
mod state;
use super::{User32CallContext32, User32Context32, User32Outcome32};
use crate::windows::user32::User32Error32;
pub(super) use state::TimerState32;

impl User32Context32 {
    pub(super) fn set_timer(
        &mut self,
        args: &[u32],
        context: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        if args[0] != 0 {
            self.owned(args[0], context.thread_id)?;
        }
        self.timers
            .set(state::TimerSet32 {
                thread: context.thread_id,
                window: args[0],
                id: args[1],
                delay: args[2],
                now: context.now_ns,
                procedure: args[3],
                activation: self.call_activation.clone(),
            })
            .map(User32Outcome32::Return)
    }
    pub(super) fn kill_timer(
        &mut self,
        args: &[u32],
        context: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        if args[0] != 0 {
            self.owned(args[0], context.thread_id)?;
        }
        Ok(User32Outcome32::Return(u32::from(self.timers.kill(
            context.thread_id,
            args[0],
            args[1],
        ))))
    }
    pub(super) fn observe_queue_with_timers(&mut self, thread: u32, mask: u32) {
        self.state.observe_queue(thread, mask);
        self.timers.observe(thread, mask);
    }
}

#[cfg(test)]
mod tests;
