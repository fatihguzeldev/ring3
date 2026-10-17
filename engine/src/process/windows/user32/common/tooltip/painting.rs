use super::*;
use crate::{
    process::windows::gdi::CanvasText32,
    windows::gdi::{GdiContext32, Region32},
};
impl User32Context32 {
    pub(in crate::process::windows::user32::common::tooltip) fn tooltip_extent(
        &self,
        window: WindowHandle32,
        tool: &Tool32,
        ram: &AddressSpace,
        gdi: &mut GdiRuntime32,
        thread: u32,
    ) -> Result<(u32, u32), HostError> {
        let tip = self.tooltip(window).map_err(HostError::User32)?;
        let font = if tip.font == 0 {
            GdiContext32::stock_object(17).map_err(HostError::Gdi)?
        } else {
            tip.font
        };
        let projection = self
            .gdi_window(window.get(), false)
            .map_err(HostError::User32)?;
        let dc = gdi
            .begin_window_paint_dc(projection, Region32::default(), font, gdi.system_color(24))
            .map_err(HostError::Gdi)?;
        let result = (|| {
            let mut canvas = gdi.begin_canvas(ram, dc).map_err(HostError::Gdi)?;
            let shaped = canvas
                .with_text_style(gdi, font, tool.flags & 4 != 0, None, |canvas| {
                    canvas.measure_text(gdi, &tool.text)
                })
                .map_err(HostError::Gdi)?;
            let width = (shaped.advance.ceil() as i32)
                .saturating_add(tip.margin.left)
                .saturating_add(tip.margin.right)
                .saturating_add(6)
                .max(1);
            let height = shaped
                .metrics
                .height
                .saturating_add(tip.margin.top)
                .saturating_add(tip.margin.bottom)
                .saturating_add(4)
                .max(1);
            Ok((width as u32, height as u32))
        })();
        assert!(
            gdi.release_window_dc(window.get(), dc, thread)
                .expect("tooltip layout owns its exact dc")
        );
        result
    }
    pub(in crate::process::windows::user32::common::tooltip) fn paint_tooltip(
        &mut self,
        window: WindowHandle32,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
        thread: u32,
    ) -> Result<User32Outcome32, HostError> {
        let (region, _) = self
            .state
            .update_region(window)
            .map_err(HostError::User32)?;
        if region.complexity() == 1 {
            return Ok(User32Outcome32::Return(0));
        }
        self.state.check_paint_change().map_err(HostError::User32)?;
        let projection = self
            .gdi_window(window.get(), false)
            .map_err(HostError::User32)?;
        let tip = self.tooltip(window).map_err(HostError::User32)?;
        let Some(tool) = tip.current() else {
            self.state
                .set_update_region(window, Region32::default(), false)
                .expect("tooltip update was preflighted");
            return Ok(User32Outcome32::Return(0));
        };
        let font = if tip.font == 0 {
            GdiContext32::stock_object(17).map_err(HostError::Gdi)?
        } else {
            tip.font
        };
        let background = if tip.background == 0xff000000 {
            gdi.system_color(24)
        } else {
            tip.background
        };
        let foreground = if tip.foreground == 0xff000000 {
            gdi.system_color(23)
        } else {
            tip.foreground
        };
        let dc = gdi
            .begin_window_paint_dc(projection, region, font, background)
            .map_err(HostError::Gdi)?;
        let mnemonic = self.state.record(window).map_err(HostError::User32)?.style & 2 == 0;
        let result = (|| {
            let mut canvas = gdi.begin_canvas(ram, dc).map_err(HostError::Gdi)?;
            let bounds = Rect32 {
                left: 0,
                top: 0,
                right: projection.width as i32,
                bottom: projection.height as i32,
            };
            canvas.fill(gdi, bounds, background);
            canvas.frame(gdi, bounds, gdi.system_color(6), gdi.system_color(6));
            canvas
                .with_text_style(gdi, font, tool.flags & 4 != 0, Some(bounds), |canvas| {
                    canvas.text(
                        gdi,
                        ram,
                        CanvasText32 {
                            units: &tool.text,
                            bounds: Rect32 {
                                left: 3 + tip.margin.left,
                                top: 2 + tip.margin.top,
                                right: bounds.right - 3 - tip.margin.right,
                                bottom: bounds.bottom - 2 - tip.margin.bottom,
                            },
                            color: foreground,
                            background: None,
                            horizontal: 0,
                            middle: false,
                            mnemonic,
                            multiline: false,
                        },
                    )
                })
                .map_err(HostError::Gdi)?;
            gdi.publish_canvas(ram, canvas).map_err(HostError::Gdi)
        })();
        assert!(
            gdi.release_window_dc(window.get(), dc, thread)
                .expect("tooltip paint owns its exact dc")
        );
        result?;
        self.state
            .set_update_region(window, Region32::default(), false)
            .expect("tooltip update was preflighted");
        Ok(User32Outcome32::Return(0))
    }
}
