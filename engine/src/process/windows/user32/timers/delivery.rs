use super::super::{User32Context32, memory};
use crate::{
    memory::AddressSpace,
    windows::{
        user32::{Message32, User32Error32},
        window::WindowHandle32,
    },
};
pub(in crate::process::windows::user32) struct TimerMessageQuery32 {
    pub output: u32,
    pub thread: u32,
    pub window: u32,
    pub min: u32,
    pub max: u32,
    pub remove: bool,
    pub mask: u32,
}
impl User32Context32 {
    fn timer_window_matches(&self, window: u32, filter: u32) -> bool {
        if filter == 0 {
            return true;
        }
        if filter == u32::MAX {
            return window == 0;
        }
        let mut window = window;
        for _ in 0..=self.timers.window_limit {
            if window == filter {
                return true;
            }
            let Some(parent) = WindowHandle32::try_from(window)
                .ok()
                .and_then(|window| self.state.record(window).ok())
                .and_then(|record| record.parent)
            else {
                return false;
            };
            window = parent.get();
        }
        false
    }
    pub(in crate::process::windows::user32) fn timer_message_ready(
        &self,
        thread: u32,
        window: u32,
        min: u32,
        max: u32,
    ) -> bool {
        if !(min == 0 && max == 0 || (min..=max).contains(&0x113)) {
            return false;
        }
        self.timers
            .pending(|message| {
                message.thread_id == thread && self.timer_window_matches(message.window, window)
            })
            .is_some()
            || self
                .timers
                .due(|timer| {
                    timer.thread == thread && self.timer_window_matches(timer.window, window)
                })
                .is_some()
    }
    pub(in crate::process::windows::user32) fn retrieve_timer_message(
        &mut self,
        memory: &mut AddressSpace,
        query: TimerMessageQuery32,
    ) -> Result<Option<Message32>, User32Error32> {
        let TimerMessageQuery32 {
            output,
            thread,
            window,
            min,
            max,
            remove,
            mask,
        } = query;
        if mask & 0x10 == 0 || !(min == 0 && max == 0 || (min..=max).contains(&0x113)) {
            return Ok(None);
        }
        if let Some((index, message)) = self.timers.pending(|message| {
            message.thread_id == thread && self.timer_window_matches(message.window, window)
        }) {
            let (sequence, activation) = self.timers.pending_activation(index);
            let delivery = self.provenance.prepare(
                super::super::messages::MessageIdentity32 {
                    timer: true,
                    sequence,
                },
                message,
                activation,
            )?;
            memory::write_message(memory, output, message)?;
            self.provenance.commit(delivery);
            if remove {
                self.timers.remove_pending(index);
            }
            return Ok(Some(message));
        }
        let Some((index, timer)) = self
            .timers
            .due(|timer| timer.thread == thread && self.timer_window_matches(timer.window, window))
        else {
            return Ok(None);
        };
        let deadline = self.timers.prepare_message(index, !remove)?;
        let message = Message32 {
            window: timer.window,
            message: 0x113,
            wparam: timer.id,
            lparam: timer.procedure,
            time_ms: self.timers.tick,
            point: self.input.query_point,
            thread_id: thread,
        };
        let delivery = self.provenance.prepare(
            super::super::messages::MessageIdentity32 {
                timer: true,
                sequence: self.timers.next_message_identity(),
            },
            message,
            timer.activation,
        )?;
        memory::write_message(memory, output, message)?;
        self.provenance.commit(delivery);
        self.timers
            .commit_message(index, deadline, (!remove).then_some(message));
        Ok(Some(message))
    }
}
