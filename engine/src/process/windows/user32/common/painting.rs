use super::super::{User32Context32, User32Outcome32};
use crate::{
    memory::AddressSpace,
    process::{HostError, windows::gdi::GdiRuntime32},
    windows::{
        comctl32::progress::Progress32,
        gdi::Region32,
        user32::{Rect32, User32Error32 as E},
        window::WindowHandle32,
    },
};

pub(super) struct ProgressVisual32 {
    pub window: WindowHandle32,
    pub progress: Progress32,
    pub thread: u32,
}

impl User32Context32 {
    pub(super) fn paint_common_progress(
        &mut self,
        visual: ProgressVisual32,
        external: u32,
        print: bool,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
    ) -> Result<User32Outcome32, HostError> {
        let ProgressVisual32 {
            window,
            progress,
            thread,
        } = visual;
        if external != 0 || print {
            if external == 0 {
                return Err(HostError::User32(E::Win32(6)));
            }
            let extent = self
                .windows()
                .get(window)
                .map_err(E::from)
                .map_err(HostError::User32)?
                .description
                .extent;
            draw_progress(
                gdi,
                ram,
                external,
                progress,
                self.state.record(window).map_err(HostError::User32)?.style,
                Rect32 {
                    left: 0,
                    top: 0,
                    right: extent.width as i32,
                    bottom: extent.height as i32,
                },
            )?;
            return Ok(User32Outcome32::Return(0));
        }
        let (region, _) = self
            .state
            .update_region(window)
            .map_err(HostError::User32)?;
        if region.complexity() == 1 {
            return Ok(User32Outcome32::Return(0));
        }
        self.state.check_paint_change().map_err(HostError::User32)?;
        self.render_common_progress(
            ProgressVisual32 {
                window,
                progress,
                thread,
            },
            region,
            ram,
            gdi,
        )?;
        self.state
            .set_update_region(window, Region32::default(), false)
            .expect("progress paint was preflighted");
        Ok(User32Outcome32::Return(0))
    }

    pub(super) fn render_common_progress(
        &self,
        visual: ProgressVisual32,
        region: Region32,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
    ) -> Result<(), HostError> {
        let ProgressVisual32 {
            window,
            progress,
            thread,
        } = visual;
        let projection = self
            .gdi_window(window.get(), false)
            .map_err(HostError::User32)?;
        let color = background(gdi, progress);
        let style = self.state.record(window).map_err(HostError::User32)?.style;
        let dc = gdi
            .begin_window_paint_dc(projection, region, 0, color)
            .map_err(HostError::Gdi)?;
        let result = draw_progress(
            gdi,
            ram,
            dc,
            progress,
            style,
            Rect32 {
                left: 0,
                top: 0,
                right: projection.width as i32,
                bottom: projection.height as i32,
            },
        );
        assert!(
            gdi.release_window_dc(window.get(), dc, thread)
                .expect("progress owns its acquired dc")
        );
        result
    }
}

fn background(gdi: &GdiRuntime32, progress: Progress32) -> u32 {
    if progress.background == 0xff00_0000 {
        gdi.system_color(15)
    } else {
        progress.background
    }
}

fn draw_progress(
    gdi: &mut GdiRuntime32,
    ram: &mut AddressSpace,
    dc: u32,
    progress: Progress32,
    style: u32,
    bounds: Rect32,
) -> Result<(), HostError> {
    let background = background(gdi, progress);
    let bar = if progress.bar == 0xff00_0000 {
        gdi.system_color(13)
    } else {
        progress.bar
    };
    let mut canvas = gdi.begin_canvas(ram, dc).map_err(HostError::Gdi)?;
    canvas.fill(gdi, bounds, background);
    let content = Rect32 {
        left: (bounds.left + 1).min(bounds.right),
        top: (bounds.top + 1).min(bounds.bottom),
        right: (bounds.right - 1).max(bounds.left),
        bottom: (bounds.bottom - 1).max(bounds.top),
    };
    if content.left < content.right
        && content.top < content.bottom
        && progress.maximum > progress.minimum
    {
        let vertical = style & 4 != 0;
        let length = if vertical {
            content.bottom - content.top
        } else {
            content.right - content.left
        };
        let span = i64::from(progress.maximum) - i64::from(progress.minimum);
        let value = (i64::from(progress.position) - i64::from(progress.minimum)).clamp(0, span);
        let mut filled = ((value * i64::from(length) + span / 2) / span) as i32;
        let smooth = style & 1 != 0;
        let thickness = if vertical {
            content.right - content.left
        } else {
            content.bottom - content.top
        };
        let chunk = ((thickness * 2 + 1) / 3).max(1);
        if !smooth {
            filled = (((filled + chunk + 1) / (chunk + 2)) * (chunk + 2)).min(length);
        }
        let mut at = 0;
        while at < filled {
            let end = if smooth {
                filled
            } else {
                (at + chunk).min(filled)
            };
            canvas.fill(
                gdi,
                if vertical {
                    Rect32 {
                        top: content.bottom - end,
                        bottom: content.bottom - at,
                        ..content
                    }
                } else {
                    Rect32 {
                        left: content.left + at,
                        right: content.left + end,
                        ..content
                    }
                },
                bar,
            );
            at = if smooth { filled } else { end + 2 };
        }
    }
    gdi.publish_canvas(ram, canvas).map_err(HostError::Gdi)
}
