use super::*;
use crate::windows::gdi::Region32;

impl User32State32 {
    pub(crate) fn take_background_erase(
        &mut self,
        window: WindowHandle32,
    ) -> Result<(), User32Error32> {
        self.windows.get(window)?;
        let slot = self
            .slots
            .iter_mut()
            .find(|slot| slot.data.handle == window)
            .ok_or(User32Error32::Win32(1400))?;
        slot.erase = false;
        Ok(())
    }
    pub(crate) fn finish_background_erase(&mut self, window: WindowHandle32, handled: bool) {
        if !handled
            && let Some(slot) = self
                .slots
                .iter_mut()
                .find(|slot| slot.data.handle == window)
            && slot.update.complexity() != 1
        {
            slot.erase = true;
        }
    }
    pub(crate) fn window_class(
        &self,
        window: WindowHandle32,
    ) -> Result<&WindowClass32, User32Error32> {
        let data = self.record(window)?;
        self.class(data.instance, &[], Some(data.class_atom))
    }
    pub(crate) fn update_region(
        &self,
        window: WindowHandle32,
    ) -> Result<(Region32, bool), User32Error32> {
        self.windows.get(window)?;
        let slot = self
            .slots
            .iter()
            .find(|slot| slot.data.handle == window)
            .ok_or(User32Error32::Win32(1400))?;
        Ok((slot.update.clone(), slot.erase))
    }
    pub(crate) fn set_update_region(
        &mut self,
        window: WindowHandle32,
        region: Region32,
        erase: bool,
    ) -> Result<(), User32Error32> {
        self.windows.get(window)?;
        let index = self
            .slots
            .iter()
            .position(|slot| slot.data.handle == window)
            .ok_or(User32Error32::Win32(1400))?;
        self.queue.check_external_change()?;
        let empty = region.complexity() == 1;
        self.slots[index].update = region;
        self.slots[index].erase = !empty && erase;
        self.slots[index].paint_seen = false;
        self.queue.commit_external_change();
        Ok(())
    }
    pub(crate) fn check_paint_change(&self) -> Result<(), User32Error32> {
        self.queue.check_external_change()
    }
    pub(crate) fn paint_status(&self, thread: u32) -> (u32, u32) {
        let mut current = 0;
        let mut fresh = 0;
        for slot in &self.slots {
            if slot.update.complexity() != 1
                && self.windows.get(slot.data.handle).is_ok_and(|window| {
                    window.description.thread_id == thread && window.description.visible
                })
            {
                current |= 0x20;
                if !slot.paint_seen {
                    fresh |= 0x20;
                }
            }
        }
        (current, fresh)
    }
    pub(super) fn with_paint_status(
        &self,
        thread: u32,
        status: (u32, u32, u64),
    ) -> (u32, u32, u64) {
        let paint = self.paint_status(thread);
        (status.0 | paint.0, status.1 | paint.1, status.2)
    }
    pub(crate) fn paint_message(
        &self,
        thread: u32,
        filter: u32,
        min: u32,
        max: u32,
        mask: u32,
        time: u32,
        point: Point32,
    ) -> Result<Option<Message32>, User32Error32> {
        if mask & 0x20 == 0 || (min != 0 || max != 0) && !(min..=max).contains(&0x0f) {
            return Ok(None);
        }
        if filter != 0 && filter != u32::MAX {
            self.windows.get(WindowHandle32::try_from(filter)?)?;
        }
        Ok(self
            .slots
            .iter()
            .find(|slot| {
                slot.update.complexity() != 1
                    && self.windows.get(slot.data.handle).is_ok_and(|window| {
                        window.description.thread_id == thread && window.description.visible
                    })
                    && (filter == 0
                        || filter != u32::MAX && self.is_descendant(slot.data.handle.get(), filter))
            })
            .map(|slot| Message32 {
                window: slot.data.handle.get(),
                message: 0x0f,
                wparam: 0,
                lparam: 0,
                time_ms: time,
                point,
                thread_id: thread,
            }))
    }
}
