use super::super::User32Context32;
use crate::windows::user32::User32Error32;

pub(in crate::process) struct PreparedUser32ThreadExit32 {
    thread: u32,
}
impl User32Context32 {
    pub(in crate::process) fn prepare_thread_termination(
        &self,
        thread: u32,
    ) -> Result<PreparedUser32ThreadExit32, User32Error32> {
        if thread == 0 {
            return Err(User32Error32::InvalidRequest);
        }
        Ok(PreparedUser32ThreadExit32 { thread })
    }
    pub(in crate::process) fn commit_thread_termination(
        &mut self,
        prepared: PreparedUser32ThreadExit32,
    ) {
        let thread = prepared.thread;
        let state = &self.state;
        self.raw_input.retire_registrations(|raw| {
            crate::windows::window::WindowHandle32::try_from(raw)
                .ok()
                .and_then(|window| state.windows().get(window).ok())
                .is_some_and(|window| window.description.thread_id == thread)
        });
        self.raw_input.retire_thread(thread);
        self.state.retire_thread(thread);
        self.timers.retire_thread(thread);
        self.provenance.retire_thread(thread);
        self.layouts.retain(|layout| layout.thread != thread);
        if self.runtime_input.thread_id == thread {
            self.pending.clear();
            self.destructions.clear();
            self.input.queued = [0; 256];
            self.input.dead = None;
        }
        self.input.focus = self.windows().focus().map_or(0, |window| window.get());
        self.input.capture = self.windows().capture().map_or(0, |window| window.get());
    }
}
