use super::{PaintOutput32, User32Context32, User32Outcome32};
use crate::{
    memory::AddressSpace,
    process::{HostError, windows::gdi::GdiRuntime32},
    windows::{
        user32::{Rect32, User32Error32},
        window::WindowHandle32,
    },
};

#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct QueryPaint32 {
    pub window: WindowHandle32,
    pub dc: u32,
    thread: u32,
    rectangle: Rect32,
    output: Option<PaintOutput32>,
}
impl QueryPaint32 {
    pub(in crate::process::windows::user32) fn check_output(
        self,
        memory: &AddressSpace,
    ) -> Result<(), User32Error32> {
        if let Some(output) = self.output {
            output.check(memory)?;
        }
        Ok(())
    }
}
impl User32Context32 {
    pub(in crate::process) fn gdi_prepare_query_output(
        memory: &mut AddressSpace,
        raw: u32,
    ) -> Result<Option<PaintOutput32>, User32Error32> {
        (raw != 0)
            .then(|| PaintOutput32::capture(memory, raw, 16))
            .transpose()
    }
    pub(in crate::process) fn gdi_begin_query_erase(
        &mut self,
        window: WindowHandle32,
        dc: u32,
        thread: u32,
        rectangle: Rect32,
        output: Option<PaintOutput32>,
    ) -> Result<User32Outcome32, User32Error32> {
        self.state.take_background_erase(window)?;
        self.start(super::Continuation32::QueryPaint(QueryPaint32 {
            window,
            dc,
            thread,
            rectangle,
            output,
        }))
    }
    pub(in crate::process::windows::user32) fn complete_query_paint(
        &mut self,
        paint: QueryPaint32,
        result: u32,
        memory: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
    ) -> Result<User32Outcome32, HostError> {
        paint.check_output(memory).map_err(HostError::User32)?;
        let finish = gdi
            .prepare_query_erase_finish(paint.window.get(), paint.dc, paint.thread)
            .map_err(HostError::Gdi)?;
        if let Some(output) = paint.output {
            let mut bytes = [0; 16];
            for (part, value) in bytes.chunks_exact_mut(4).zip([
                paint.rectangle.left,
                paint.rectangle.top,
                paint.rectangle.right,
                paint.rectangle.bottom,
            ]) {
                part.copy_from_slice(&value.to_le_bytes());
            }
            memory
                .write(output.address, &bytes)
                .map_err(HostError::Memory)?;
        }
        gdi.commit_query_erase_finish(finish);
        self.state
            .finish_background_erase(paint.window, result != 0);
        self.pending.pop();
        Ok(User32Outcome32::Return(1))
    }
}
