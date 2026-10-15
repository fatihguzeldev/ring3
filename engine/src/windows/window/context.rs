use std::collections::VecDeque;

use super::{
    WindowDescription32, WindowError32, WindowExtent32, WindowHandle32, WindowMessage32,
    WindowRecord32,
};

pub struct WindowContext32 {
    windows: Vec<WindowRecord32>,
    messages: VecDeque<WindowMessage32>,
    window_limit: usize,
    message_limit: usize,
    next_handle: u32,
    focus: Option<WindowHandle32>,
    capture: Option<WindowHandle32>,
}

impl WindowContext32 {
    pub fn new(max_windows: usize, max_messages: usize) -> Result<Self, WindowError32> {
        if !(1..=65_536).contains(&max_windows) || !(1..=65_536).contains(&max_messages) {
            return Err(WindowError32::InvalidLimits);
        }
        let mut windows = Vec::new();
        windows
            .try_reserve_exact(max_windows)
            .map_err(|_| WindowError32::Allocation)?;
        let mut messages = VecDeque::new();
        messages
            .try_reserve_exact(max_messages)
            .map_err(|_| WindowError32::Allocation)?;
        Ok(Self {
            windows,
            messages,
            window_limit: max_windows,
            message_limit: max_messages,
            next_handle: 0,
            focus: None,
            capture: None,
        })
    }

    pub fn create(
        &mut self,
        description: WindowDescription32,
    ) -> Result<WindowHandle32, WindowError32> {
        let handle = self.check_create(description)?;
        self.windows.push(WindowRecord32 {
            handle,
            description,
        });
        self.next_handle = handle.get();
        Ok(handle)
    }

    pub(crate) fn check_create(
        &self,
        description: WindowDescription32,
    ) -> Result<WindowHandle32, WindowError32> {
        description.extent.validate()?;
        if description.thread_id == 0 {
            return Err(WindowError32::InvalidThread);
        }
        if self.windows.len() == self.window_limit {
            return Err(WindowError32::Capacity);
        }
        let next_handle = self
            .next_handle
            .checked_add(1)
            .ok_or(WindowError32::Exhausted)?;
        Ok(WindowHandle32::new(next_handle))
    }

    pub(crate) fn update_description(
        &mut self,
        handle: WindowHandle32,
        description: WindowDescription32,
    ) -> Result<(), WindowError32> {
        description.extent.validate()?;
        let index = self.index(handle)?;
        if self.windows[index].description.thread_id != description.thread_id {
            return Err(WindowError32::InvalidThread);
        }
        self.windows[index].description = description;
        Ok(())
    }

    pub(crate) fn remove_delivered(
        &mut self,
        thread_id: u32,
        handle: WindowHandle32,
    ) -> Result<(), WindowError32> {
        self.owned(thread_id, handle)?;
        if self.focus == Some(handle) || self.capture == Some(handle) {
            return Err(WindowError32::InvalidHandle);
        }
        self.windows.remove(self.index(handle)?);
        Ok(())
    }

    pub fn get(&self, handle: WindowHandle32) -> Result<WindowRecord32, WindowError32> {
        Ok(self.windows[self.index(handle)?])
    }

    pub fn live_windows(&self) -> usize {
        self.windows.len()
    }

    pub fn focus(&self) -> Option<WindowHandle32> {
        self.focus
    }

    pub fn capture(&self) -> Option<WindowHandle32> {
        self.capture
    }

    pub fn resize(
        &mut self,
        handle: WindowHandle32,
        extent: WindowExtent32,
    ) -> Result<(), WindowError32> {
        extent.validate()?;
        let index = self.index(handle)?;
        if self.windows[index].description.extent == extent {
            return Ok(());
        }
        self.check_messages(1)?;
        self.windows[index].description.extent = extent;
        self.push(
            self.windows[index],
            0x0005,
            0,
            (extent.width & 0xffff) | (extent.height << 16),
        );
        Ok(())
    }

    pub fn set_focus(
        &mut self,
        thread_id: u32,
        handle: Option<WindowHandle32>,
    ) -> Result<Option<WindowHandle32>, WindowError32> {
        self.check_set_focus(thread_id, handle)?;
        let next = handle
            .map(|handle| self.owned(thread_id, handle))
            .transpose()?;
        let old = self.focus;
        if old == handle {
            return Ok(old);
        }
        let previous = old.map(|handle| self.get(handle)).transpose()?;
        if let Some(previous) = previous {
            self.push(previous, 0x0008, raw(handle), 0);
        }
        if let Some(next) = next {
            self.push(next, 0x0007, raw(old), 0);
        }
        self.focus = handle;
        Ok(old)
    }

    pub fn check_set_focus(
        &self,
        thread_id: u32,
        handle: Option<WindowHandle32>,
    ) -> Result<(), WindowError32> {
        let next = handle
            .map(|handle| self.owned(thread_id, handle))
            .transpose()?;
        self.check_thread(thread_id, self.focus)?;
        if next.is_some_and(|window| !window.description.enabled) {
            return Err(WindowError32::Disabled);
        }
        let count = if self.focus == handle {
            0
        } else {
            usize::from(self.focus.is_some()) + usize::from(handle.is_some())
        };
        self.check_messages(count)
    }

    pub fn set_capture(
        &mut self,
        thread_id: u32,
        handle: Option<WindowHandle32>,
    ) -> Result<Option<WindowHandle32>, WindowError32> {
        self.check_set_capture(thread_id, handle)?;
        let old = self.capture;
        if old == handle {
            return Ok(old);
        }
        let previous = old.map(|handle| self.get(handle)).transpose()?;
        if let Some(previous) = previous {
            self.push(previous, 0x0215, 0, raw(handle));
        }
        self.capture = handle;
        Ok(old)
    }

    pub fn check_set_capture(
        &self,
        thread_id: u32,
        handle: Option<WindowHandle32>,
    ) -> Result<(), WindowError32> {
        if let Some(handle) = handle {
            self.owned(thread_id, handle)?;
        }
        self.check_thread(thread_id, self.capture)?;
        self.check_messages(usize::from(
            self.capture != handle && self.capture.is_some(),
        ))
    }

    pub fn release_capture(&mut self, thread_id: u32) -> Result<(), WindowError32> {
        self.set_capture(thread_id, None)?;
        Ok(())
    }

    pub fn destroy(
        &mut self,
        thread_id: u32,
        handle: WindowHandle32,
    ) -> Result<WindowRecord32, WindowError32> {
        self.check_destroy(thread_id, handle)?;
        let window = self.owned(thread_id, handle)?;
        let captured = self.capture == Some(handle);
        let focused = self.focus == Some(handle);
        if captured {
            self.push(window, 0x0215, 0, 0);
            self.capture = None;
        }
        if focused {
            self.push(window, 0x0008, 0, 0);
            self.focus = None;
        }
        self.push(window, 0x0002, 0, 0);
        self.push(window, 0x0082, 0, 0);
        self.windows.remove(self.index(handle)?);
        Ok(window)
    }

    pub fn check_destroy(
        &self,
        thread_id: u32,
        handle: WindowHandle32,
    ) -> Result<(), WindowError32> {
        self.owned(thread_id, handle)?;
        self.check_messages(
            2 + usize::from(self.capture == Some(handle)) + usize::from(self.focus == Some(handle)),
        )
    }

    pub fn queued_messages(&self) -> usize {
        self.messages.len()
    }

    pub fn peek_message(&self) -> Option<WindowMessage32> {
        self.messages.front().copied()
    }

    pub fn pop_message(&mut self) -> Option<WindowMessage32> {
        self.messages.pop_front()
    }

    fn index(&self, handle: WindowHandle32) -> Result<usize, WindowError32> {
        self.windows
            .binary_search_by_key(&handle, |window| window.handle)
            .map_err(|_| WindowError32::InvalidHandle)
    }

    fn owned(
        &self,
        thread_id: u32,
        handle: WindowHandle32,
    ) -> Result<WindowRecord32, WindowError32> {
        let window = self.get(handle)?;
        if thread_id == 0 || window.description.thread_id != thread_id {
            return Err(WindowError32::InvalidThread);
        }
        Ok(window)
    }

    fn check_thread(
        &self,
        thread_id: u32,
        current: Option<WindowHandle32>,
    ) -> Result<(), WindowError32> {
        if thread_id == 0 {
            return Err(WindowError32::InvalidThread);
        }
        if let Some(handle) = current {
            self.owned(thread_id, handle)?;
        }
        Ok(())
    }

    fn check_messages(&self, count: usize) -> Result<(), WindowError32> {
        if count > self.message_limit - self.messages.len() {
            return Err(WindowError32::QueueFull);
        }
        Ok(())
    }

    fn push(&mut self, window: WindowRecord32, message: u32, wparam: u32, lparam: u32) {
        self.messages.push_back(WindowMessage32 {
            window: window.handle,
            procedure: window.description.procedure,
            message,
            wparam,
            lparam,
        });
    }
}

fn raw(handle: Option<WindowHandle32>) -> u32 {
    handle.map_or(0, WindowHandle32::get)
}

#[cfg(test)]
mod tests;
