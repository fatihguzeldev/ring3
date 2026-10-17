use super::{User32Context32, User32Outcome32, callback::Continuation32};
mod query;
mod sending;
#[cfg(test)]
mod tests;
mod visibility;
use crate::windows::{
    gdi::GdiWindow32,
    user32::{Rect32, User32Error32},
    window::WindowHandle32,
};
pub(super) use query::QueryPaint32;
pub(super) use sending::SentPaint32;
impl User32Context32 {
    pub(in crate::process) fn gdi_update_region(
        &self,
        window: WindowHandle32,
    ) -> Result<(crate::windows::gdi::Region32, bool), User32Error32> {
        self.state.update_region(window)
    }
    pub(in crate::process) fn gdi_set_update_region(
        &mut self,
        window: WindowHandle32,
        region: crate::windows::gdi::Region32,
        erase: bool,
    ) -> Result<(), User32Error32> {
        self.state.set_update_region(window, region, erase)
    }
    pub(in crate::process) fn gdi_check_begin_paint(
        &mut self,
        window: WindowHandle32,
        erase: bool,
        thread: u32,
    ) -> Result<(), User32Error32> {
        let owner = self.windows().get(window)?.description.thread_id;
        self.state.check_paint_change()?;
        if erase {
            self.prepare_thread_slots()?;
            self.check_continuation()?;
            if owner != thread {
                self.check_sent_paint_capacity()?;
            }
        }
        Ok(())
    }
    pub(in crate::process) fn gdi_prepare_paint_output(
        memory: &mut crate::memory::AddressSpace,
        raw: u32,
    ) -> Result<PaintOutput32, User32Error32> {
        PaintOutput32::new(memory, raw)
    }
    pub(in crate::process) fn gdi_begin_paint(
        &mut self,
        window: WindowHandle32,
        output: PaintOutput32,
        dc: u32,
        erase: bool,
        thread: u32,
    ) -> Result<User32Outcome32, User32Error32> {
        self.state
            .set_update_region(window, crate::windows::gdi::Region32::default(), false)?;
        if erase && self.windows().get(window)?.description.thread_id != thread {
            self.start_sent_paint(window, thread, Some((output, dc)))
        } else if erase {
            self.start(Continuation32::Paint { window, output, dc })
        } else {
            Ok(User32Outcome32::Return(dc))
        }
    }
    pub(in crate::process) fn gdi_update_window(
        &mut self,
        window: WindowHandle32,
        thread: u32,
    ) -> Result<User32Outcome32, User32Error32> {
        let (region, _) = self.state.update_region(window)?;
        if region.complexity() == 1 {
            return Ok(User32Outcome32::Return(1));
        }
        self.prepare_thread_slots()?;
        self.check_continuation()?;
        if self.windows().get(window)?.description.thread_id != thread {
            self.check_sent_paint_capacity()?;
            return self.start_sent_paint(window, thread, None);
        }
        let procedure = self.windows().get(window)?.description.procedure;
        self.start(Continuation32::Sequence {
            events: [
                Some(crate::windows::window::WindowMessage32 {
                    window,
                    procedure,
                    message: 0x0f,
                    wparam: 0,
                    lparam: 0,
                }),
                None,
                None,
                None,
                None,
                None,
            ],
            index: 0,
            result: 1,
        })
    }
    pub(in crate::process) fn gdi_window(
        &self,
        raw: u32,
        whole: bool,
    ) -> Result<GdiWindow32, User32Error32> {
        let handle = WindowHandle32::try_from(raw)?;
        let window = self.windows().get(handle)?;
        let data = self.state.record(handle)?;
        let class = self.state.window_class(handle)?;
        let rect = if whole {
            data.outer
        } else {
            Rect32 {
                left: 0,
                top: 0,
                right: window.description.extent.width as i32,
                bottom: window.description.extent.height as i32,
            }
        };
        if data.destroying {
            return Err(User32Error32::Win32(1400));
        }
        Ok(GdiWindow32 {
            window: raw,
            thread: window.description.thread_id,
            class_atom: class.atom,
            class_style: class.style,
            background_brush: class.background_brush,
            width: (rect.right - rect.left) as u32,
            height: (rect.bottom - rect.top) as u32,
            whole,
        })
    }
}

#[derive(Clone, Copy)]
pub(in crate::process) struct PaintOutput32 {
    address: crate::memory::GuestAddress,
    identity: u64,
    first: u32,
    mappings: [u64; 2],
    count: u32,
    length: usize,
}
impl PaintOutput32 {
    fn new(memory: &mut crate::memory::AddressSpace, raw: u32) -> Result<Self, User32Error32> {
        Self::capture(memory, raw, 64)
    }
    fn capture(
        memory: &mut crate::memory::AddressSpace,
        raw: u32,
        length: usize,
    ) -> Result<Self, User32Error32> {
        use crate::memory::{Access, GuestAddress, PAGE_SIZE};
        let address = GuestAddress(raw);
        memory.check_span(address, length, Access::Write)?;
        let first = raw / PAGE_SIZE;
        let last = ((u64::from(raw) + length as u64 - 1) / u64::from(PAGE_SIZE)) as u32;
        let mut mappings = [0; 2];
        memory.observe_mapping_incarnations(crate::memory::PageRange::new(
            GuestAddress(first * PAGE_SIZE),
            last - first + 1,
        )?)?;
        for page in first..=last {
            mappings[(page - first) as usize] =
                memory.mapping_incarnation_at(GuestAddress(page * PAGE_SIZE))?;
        }
        Ok(Self {
            address,
            identity: memory.identity(),
            first,
            mappings,
            count: last - first + 1,
            length,
        })
    }
    pub(super) fn check(self, memory: &crate::memory::AddressSpace) -> Result<(), User32Error32> {
        if self.identity != memory.identity() {
            return Err(User32Error32::InvalidRequest);
        }
        for page in 0..self.count {
            if memory.mapping_incarnation_at(crate::memory::GuestAddress(
                (self.first + page) * crate::memory::PAGE_SIZE,
            ))? != self.mappings[page as usize]
            {
                return Err(User32Error32::InvalidRequest);
            }
        }
        memory.check_span(self.address, self.length, crate::memory::Access::Write)?;
        Ok(())
    }
    pub(super) fn complete(
        self,
        memory: &mut crate::memory::AddressSpace,
        erased: bool,
    ) -> Result<(), User32Error32> {
        self.check(memory)?;
        memory.write(
            crate::memory::GuestAddress(self.address.0 + 4),
            &u32::from(!erased).to_le_bytes(),
        )?;
        Ok(())
    }
}
