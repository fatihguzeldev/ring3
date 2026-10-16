use super::super::User32Context32;
use crate::{input::keyboard::KeyboardLayout32, windows::user32::User32Error32};

#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct ThreadLayout32 {
    pub(super) thread: u32,
    pub query: KeyboardLayout32,
    pub delivery: KeyboardLayout32,
}
impl User32Context32 {
    pub(in crate::process::windows::user32) fn prepare_thread_layout(
        &mut self,
        thread: u32,
    ) -> Result<(), User32Error32> {
        if thread == 0 {
            return Err(User32Error32::InvalidRequest);
        }
        if self.layouts.iter().any(|layout| layout.thread == thread) {
            return Ok(());
        }
        if self.layouts.len() == 4096 {
            return Err(User32Error32::Capacity);
        }
        self.layouts
            .try_reserve(1)
            .map_err(|_| User32Error32::Allocation)?;
        self.layouts.push(ThreadLayout32 {
            thread,
            query: self.input.layout,
            delivery: self.input.layout,
        });
        Ok(())
    }
    pub(in crate::process::windows::user32) fn record_thread_layout(&mut self, thread: u32) {
        if let Some(layout) = self
            .layouts
            .iter_mut()
            .find(|layout| layout.thread == thread)
        {
            layout.query = self.input.layout;
        }
    }
    pub(in crate::process::windows::user32) fn queried_thread_layout(&self, thread: u32) -> u32 {
        if thread == 0 || thread == self.runtime_input.thread_id {
            return self.input.layout.handle();
        }
        self.layouts
            .iter()
            .find(|layout| layout.thread == thread)
            .map_or(0, |layout| layout.query.handle())
    }
    pub(in crate::process::windows::user32) fn delivered_thread_layout(
        &self,
        thread: u32,
    ) -> KeyboardLayout32 {
        self.layouts
            .iter()
            .find(|layout| layout.thread == thread)
            .map_or(self.input.delivery_layout, |layout| layout.delivery)
    }
    pub(in crate::process::windows::user32) fn commit_thread_layout_delivery(
        &mut self,
        thread: u32,
        value: KeyboardLayout32,
    ) {
        if let Some(layout) = self
            .layouts
            .iter_mut()
            .find(|layout| layout.thread == thread)
        {
            layout.delivery = value;
        }
    }
    #[cfg(test)]
    pub(in crate::process) fn resolve_thread_keyboard_layout(
        &self,
        reply: &super::User32ThreadReply32,
    ) -> u32 {
        self.resolve_thread_layout(reply).handle()
    }
    pub(super) fn resolve_thread_layout(
        &self,
        reply: &super::User32ThreadReply32,
    ) -> KeyboardLayout32 {
        self.process_layout
            .filter(|policy| policy.0 > reply.layout_revision())
            .map_or_else(|| reply.layout(), |policy| policy.1)
    }
    pub(in crate::process::windows::user32) fn activate_layout(
        &mut self,
        next: KeyboardLayout32,
        all_threads: bool,
        result: u32,
    ) -> Result<super::super::User32Outcome32, User32Error32> {
        let target = all_threads
            .then(|| self.windows().focus().or(self.windows().active()))
            .flatten()
            .and_then(|window| self.windows().get(window).ok())
            .filter(|window| window.description.thread_id == self.runtime_input.thread_id);
        if target.is_some() {
            self.check_continuation()?;
        }
        let revision = if all_threads {
            Some(
                self.process_layout
                    .map_or(0, |policy| policy.0)
                    .checked_add(1)
                    .ok_or(User32Error32::Capacity)?,
            )
        } else {
            None
        };
        self.input.layout = next;
        self.input.dead = None;
        if let Some(revision) = revision {
            self.process_layout = Some((revision, next));
            for layout in &mut self.layouts {
                layout.query = next;
                if self.input.cursor.is_none() {
                    layout.delivery = next;
                }
            }
        }
        if self.input.cursor.is_none() {
            self.input.delivery_layout = next;
        }
        let mut events = [None; 6];
        events[0] = target.map(|window| crate::windows::window::WindowMessage32 {
            window: window.handle,
            procedure: window.description.procedure,
            message: 0x51,
            wparam: 0,
            lparam: next.handle(),
        });
        self.sequence(events, result)
    }
    pub(in crate::process) fn reset_thread_layout_delivery(&mut self) {
        for layout in &mut self.layouts {
            layout.delivery = layout.query;
        }
    }
}
