use super::super::{User32CallContext32, User32Context32, User32Outcome32, memory};
use crate::{
    memory::AddressSpace,
    process::{HostError, windows::gdi::GdiRuntime32},
    windows::{
        comctl32::{CommonControlState32 as State, trackbar::Trackbar32},
        user32::{User32Api32, User32Error32 as E},
        window::WindowHandle32,
    },
};
impl User32Context32 {
    pub(super) fn trackbar(&self, window: WindowHandle32) -> Result<&Trackbar32, E> {
        match self.state.common_state(window)? {
            State::Trackbar(value) => Ok(value),
            _ => Err(E::Win32(1407)),
        }
    }
    pub(super) fn trackbar_mut(&mut self, window: WindowHandle32) -> Result<&mut Trackbar32, E> {
        match self.state.common_state_mut(window)? {
            State::Trackbar(value) => Ok(value),
            _ => Err(E::Win32(1407)),
        }
    }
    pub(super) fn trackbar_control_message(
        &mut self,
        args: &[u32],
        wide: bool,
        ram: &mut AddressSpace,
        gdi: Option<&mut GdiRuntime32>,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, HostError> {
        let window = self
            .owned(args[0], call.thread_id)
            .map_err(HostError::User32)?;
        if matches!(args[1], 0xf | 0x318 | 0x14) {
            return self.paint_numeric(
                window,
                args[2],
                args[1],
                ram,
                gdi.ok_or(HostError::InvalidRequest)?,
                call.thread_id,
            );
        }
        if matches!(
            args[1],
            0x100 | 0x101 | 0x200..=0x203 | 0x215 | 0x1f | 0x113
        ) {
            return self
                .trackbar_input(window, args, ram, call)
                .map_err(HostError::User32);
        }
        let mut redraw = false;
        let value = match args[1] {
            0x81 => {
                let auto = self.state.record(window).map_err(HostError::User32)?.style & 1 != 0;
                let bar = self.trackbar_mut(window).map_err(HostError::User32)?;
                bar.unicode = wide;
                if auto {
                    bar.set_frequency(1).map_err(HostError::User32)?;
                }
                1
            }
            1 => 0,
            0x87 => 0x81,
            0x7 | 0x8 | 0xa => {
                redraw = true;
                0
            }
            0x30 => {
                if args[2] != 0 {
                    gdi.as_ref()
                        .ok_or(HostError::InvalidRequest)?
                        .font_metrics(args[2])
                        .map_err(HostError::Gdi)?;
                }
                if args[3] != 0 {
                    self.state.check_paint_change().map_err(HostError::User32)?;
                }
                self.trackbar_mut(window).map_err(HostError::User32)?.font = args[2];
                redraw = args[3] != 0;
                0
            }
            0x31 => self.trackbar(window).map_err(HostError::User32)?.font,
            0x400 => self.trackbar(window).map_err(HostError::User32)?.position as u32,
            0x401 => self.trackbar(window).map_err(HostError::User32)?.minimum as u32,
            0x402 => self.trackbar(window).map_err(HostError::User32)?.maximum as u32,
            0x403 => self
                .trackbar(window)
                .map_err(HostError::User32)?
                .tick(args[2] as usize)
                .map_or(u32::MAX, |v| v as u32),
            0x404 => {
                self.state.check_paint_change().map_err(HostError::User32)?;
                redraw = true;
                u32::from(
                    self.trackbar_mut(window)
                        .map_err(HostError::User32)?
                        .add_tick(args[3] as i32)
                        .map_err(HostError::User32)?,
                )
            }
            0x405 => {
                redraw = args[2] != 0;
                if redraw {
                    self.state.check_paint_change().map_err(HostError::User32)?;
                }
                let bar = self.trackbar_mut(window).map_err(HostError::User32)?;
                bar.position = bar.clamp(i64::from(args[3] as i32));
                0
            }
            0x406..=0x408 => {
                let bar = self.trackbar(window).map_err(HostError::User32)?;
                let (min, max) = match args[1] {
                    0x406 => (i32::from(args[3] as i16), i32::from((args[3] >> 16) as i16)),
                    0x407 => (args[3] as i32, bar.maximum),
                    _ => (bar.minimum, args[3] as i32),
                };
                redraw = args[2] != 0;
                if redraw {
                    self.state.check_paint_change().map_err(HostError::User32)?;
                }
                self.trackbar_mut(window)
                    .map_err(HostError::User32)?
                    .set_range(min, max)
                    .map_err(HostError::User32)?;
                0
            }
            0x409 => {
                redraw = args[2] != 0;
                if redraw {
                    self.state.check_paint_change().map_err(HostError::User32)?;
                }
                let bar = self.trackbar_mut(window).map_err(HostError::User32)?;
                bar.ticks.clear();
                bar.frequency = None;
                0
            }
            0x40a..=0x40c | 0x413 => {
                redraw = args[2] != 0;
                if redraw {
                    self.state.check_paint_change().map_err(HostError::User32)?;
                }
                let bar = self.trackbar_mut(window).map_err(HostError::User32)?;
                let selection = match args[1] {
                    0x40a => (i32::from(args[3] as i16), i32::from((args[3] >> 16) as i16)),
                    0x40b => (args[3] as i32, bar.selection.1),
                    0x40c => (bar.selection.0, args[3] as i32),
                    _ => (0, 0),
                };
                bar.selection = (
                    bar.clamp(i64::from(selection.0)),
                    bar.clamp(i64::from(selection.1)),
                );
                0
            }
            0x40f | 0x419 | 0x41a => {
                let extent = self
                    .windows()
                    .get(window)
                    .map_err(E::from)
                    .map_err(HostError::User32)?
                    .description
                    .extent;
                let vertical = self.state.record(window).map_err(HostError::User32)?.style & 2 != 0;
                let bar = self.trackbar(window).map_err(HostError::User32)?;
                let channel = bar.channel(extent.width as i32, extent.height as i32, vertical);
                if args[1] == 0x40f {
                    return Ok(User32Outcome32::Return(
                        bar.tick(args[2] as usize).map_or(u32::MAX, |value| {
                            bar.coordinate(value, channel, vertical) as u32
                        }),
                    ));
                }
                let rect = if args[1] == 0x419 {
                    bar.thumb(extent.width as i32, extent.height as i32, vertical)
                } else {
                    channel
                };
                memory::write_rect(ram, args[3], rect).map_err(HostError::User32)?;
                0
            }
            0x410 => {
                (self
                    .trackbar(window)
                    .map_err(HostError::User32)?
                    .tick_count()
                    + 2) as u32
            }
            0x411 => {
                self.trackbar(window)
                    .map_err(HostError::User32)?
                    .selection
                    .0 as u32
            }
            0x412 => {
                self.trackbar(window)
                    .map_err(HostError::User32)?
                    .selection
                    .1 as u32
            }
            0x414 => {
                self.state.check_paint_change().map_err(HostError::User32)?;
                self.trackbar_mut(window)
                    .map_err(HostError::User32)?
                    .set_frequency(args[2])
                    .map_err(HostError::User32)?;
                redraw = true;
                0
            }
            0x415 => {
                let bar = self.trackbar_mut(window).map_err(HostError::User32)?;
                let old = bar.page();
                bar.page = if args[3] == u32::MAX {
                    None
                } else if (args[3] as i32) >= 0 {
                    Some(args[3] as i32)
                } else {
                    return Err(HostError::User32(E::Win32(87)));
                };
                old as u32
            }
            0x416 => self.trackbar(window).map_err(HostError::User32)?.page() as u32,
            0x417 => {
                if (args[3] as i32) < 0 {
                    return Err(HostError::User32(E::Win32(87)));
                }
                let bar = self.trackbar_mut(window).map_err(HostError::User32)?;
                let old = bar.line;
                bar.line = args[3] as i32;
                old as u32
            }
            0x418 => self.trackbar(window).map_err(HostError::User32)?.line as u32,
            0x41b => {
                if self.state.record(window).map_err(HostError::User32)?.style & 0x40 != 0 {
                    if args[2] == 0 || args[2] > 32767 {
                        return Err(HostError::User32(E::Win32(87)));
                    }
                    self.state.check_paint_change().map_err(HostError::User32)?;
                    self.trackbar_mut(window)
                        .map_err(HostError::User32)?
                        .thumb_length = args[2] as i32;
                    redraw = true;
                }
                0
            }
            0x41c => {
                self.trackbar(window)
                    .map_err(HostError::User32)?
                    .thumb_length as u32
            }
            0x422 => {
                return self
                    .begin_trackbar_change(window, args[3] as i32, 4, false, false, ram, call)
                    .map_err(HostError::User32);
            }
            0x2005 => {
                let bar = self.trackbar_mut(window).map_err(HostError::User32)?;
                let old = bar.unicode;
                bar.unicode = args[2] != 0;
                u32::from(old)
            }
            0x2006 => u32::from(self.trackbar(window).map_err(HostError::User32)?.unicode),
            0x400..=0x4ff => return Err(HostError::User32(E::Win32(50))),
            _ => {
                return self
                    .prepare_arguments(
                        if wide {
                            User32Api32::DefWindowProcW
                        } else {
                            User32Api32::DefWindowProcA
                        },
                        args,
                        ram,
                        call,
                    )
                    .map_err(HostError::User32);
            }
        };
        if redraw {
            self.state.check_paint_change().map_err(HostError::User32)?;
            self.numeric_changed(window);
        }
        Ok(User32Outcome32::Return(value))
    }
}
