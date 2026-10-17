use super::super::User32Context32;
use crate::windows::{
    gdi::{GdiError32, GdiWindow32, Region32},
    user32::Point32,
    window::WindowHandle32,
};

impl User32Context32 {
    pub(in crate::process) fn gdi_dc_visibility(
        &self,
        raw: u32,
        whole: bool,
        children: bool,
    ) -> Result<(GdiWindow32, Point32, Region32), GdiError32> {
        let window = self
            .gdi_window(raw, whole)
            .map_err(|_| GdiError32::InvalidHandle)?;
        let handle = WindowHandle32::try_from(raw).map_err(|_| GdiError32::InvalidHandle)?;
        let data = self
            .state
            .record(handle)
            .map_err(|_| GdiError32::InvalidHandle)?;
        let origin = if whole {
            Point32 {
                x: data.outer.left,
                y: data.outer.top,
            }
        } else {
            data.client_origin
        };
        let mut region = Region32::rectangle(crate::windows::user32::Rect32 {
            left: 0,
            top: 0,
            right: window.width as i32,
            bottom: window.height as i32,
        });
        if children {
            for child in self.state.related_windows(handle, true) {
                let record = self
                    .windows()
                    .get(child)
                    .map_err(|_| GdiError32::InvalidHandle)?;
                let data = self
                    .state
                    .record(child)
                    .map_err(|_| GdiError32::InvalidHandle)?;
                if !record.description.visible || data.destroying {
                    continue;
                }
                let child = Region32::rectangle(data.outer).offset(
                    origin.x.checked_neg().ok_or(GdiError32::InvalidRequest)?,
                    origin.y.checked_neg().ok_or(GdiError32::InvalidRequest)?,
                )?;
                region = region.combine(&child, 4)?;
            }
        }
        Ok((window, origin, region))
    }
}
