use crate::{
    process::{EngineInstance, windows::gdi::memory},
    windows::gdi::{GdiApi32, GdiError32},
};
impl EngineInstance {
    pub(in crate::process::windows::gdi) fn gdi_global_pattern_call(
        &mut self,
        api: GdiApi32,
        args: &[u32],
    ) -> Option<Result<u32, GdiError32>> {
        let descriptor = match api {
            GdiApi32::CreateDIBPatternBrush => Ok([5, args[1], args[0]]),
            GdiApi32::CreateBrushIndirect => memory::words(self.memory.as_ref()?, args[0]),
            GdiApi32::ExtCreatePen => memory::words(self.memory.as_ref()?, args[2]),
            _ => return None,
        };
        let [style, usage, handle] = match descriptor {
            Ok(brush) => brush,
            Err(error) => return Some(Err(error)),
        };
        if !matches!(style, 5 | 8) {
            return None;
        }
        let source = match self.windows_global_memory(handle) {
            Ok(source) => source,
            Err(_) => return Some(Err(GdiError32::InvalidHandle)),
        };
        let runtime = match self.gdi.as_mut() {
            Some(runtime) => runtime,
            None => return Some(Err(GdiError32::InvalidConfig)),
        };
        let ram = self.memory.as_ref()?;
        Some(if api == GdiApi32::ExtCreatePen {
            runtime.ext_create_pen_with_brush(
                ram,
                args,
                [6, usage, source.pointer],
                Some(source.bytes),
            )
        } else {
            runtime.create_dib_pattern_brush_bounded(
                ram,
                source.pointer,
                usage & 0xffff,
                Some(source.bytes),
            )
        })
    }
}
