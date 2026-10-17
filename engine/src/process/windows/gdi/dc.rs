use crate::{
    process::EngineInstance,
    windows::{
        gdi::{GdiError32, GdiObject32, GdiWindow32, Region32},
        user32::{Point32, Rect32},
    },
};

impl EngineInstance {
    pub(in crate::process::windows::gdi) fn gdi_dc_ex(
        &mut self,
        args: &[u32],
        thread: u32,
    ) -> Result<u32, GdiError32> {
        let flags = args[2];
        // parent-surface drawing and sibling z-order have no canonical window authority yet.
        if flags & (0x10 | 0x20) != 0 || flags & !(1 | 2 | 4 | 8 | 0x40 | 0x80 | 0x400) != 0 {
            return Err(GdiError32::Unsupported);
        }
        if flags & 0xc0 == 0xc0 {
            return Err(GdiError32::InvalidRequest);
        }
        let whole = flags & 1 != 0;
        let (window, origin, mut region) = if args[0] == 0 {
            let config = self.gdi.as_ref().ok_or(GdiError32::InvalidConfig)?.config();
            (
                GdiWindow32 {
                    window: 0,
                    thread,
                    class_atom: 0,
                    class_style: 0,
                    background_brush: 0,
                    width: config.screen_width,
                    height: config.screen_height,
                    whole,
                },
                Point32::default(),
                Region32::rectangle(Rect32 {
                    left: 0,
                    top: 0,
                    right: config.screen_width as i32,
                    bottom: config.screen_height as i32,
                }),
            )
        } else {
            self.user32
                .as_ref()
                .ok_or(GdiError32::InvalidConfig)?
                .gdi_dc_visibility(args[0], whole, flags & 8 != 0)?
        };
        let runtime = self.gdi.as_mut().ok_or(GdiError32::InvalidConfig)?;
        let consumed = if flags & 0xc0 != 0 && args[1] != 1 {
            let clip = if args[1] == 0 {
                Region32::default()
            } else {
                let GdiObject32::Region(region) = runtime.model.object(args[1])? else {
                    return Err(GdiError32::InvalidHandle);
                };
                runtime.model.check_delete_object(args[1])?;
                region.offset(
                    origin.x.checked_neg().ok_or(GdiError32::InvalidRequest)?,
                    origin.y.checked_neg().ok_or(GdiError32::InvalidRequest)?,
                )?
            };
            region = region.combine(&clip, if flags & 0x40 != 0 { 4 } else { 1 })?;
            (args[1] != 0).then_some(args[1])
        } else {
            None
        };
        let region = std::sync::Arc::new(region);
        let dc = runtime.acquire_dc_options(window, thread, flags & 2 != 0)?;
        runtime.model.set_dc_visible_region(dc, Some(region));
        if let Some(region) = consumed {
            runtime
                .model
                .delete_object(region)
                .expect("GetDCEx preflighted the transferred region");
        }
        Ok(dc)
    }
}
