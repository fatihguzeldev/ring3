use super::super::{User32CallContext32, User32Context32, User32Outcome32};
use super::painting::ProgressVisual32;
use crate::{
    memory::{AddressSpace, GuestAddress},
    process::{HostError, windows::gdi::GdiRuntime32},
    windows::{
        comctl32::CommonControlState32,
        gdi::Region32,
        user32::{Rect32, User32Error32 as E},
    },
};

impl User32Context32 {
    pub(super) fn common_control_message(
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
        let before = match self.state.common_state(window).map_err(HostError::User32)? {
            CommonControlState32::Progress(progress) => *progress,
            CommonControlState32::Tooltip(_) => {
                return self.tooltip_control_message(args, wide, ram, gdi, call);
            }
            CommonControlState32::Tab(_) => {
                return self.tab_control_message(args, wide, ram, gdi, call);
            }
            CommonControlState32::Hotkey(_) => {
                return self.hotkey_control_message(args, wide, ram, gdi, call);
            }
            CommonControlState32::Status(_) => {
                return self.status_control_message(args, wide, ram, gdi, call);
            }
        };
        if args[1] == 0x0f || args[1] == 0x318 {
            return self.paint_common_progress(
                ProgressVisual32 {
                    window,
                    progress: before,
                    thread: call.thread_id,
                },
                args[2],
                args[1] == 0x318,
                ram,
                gdi.ok_or(HostError::InvalidRequest)?,
            );
        }
        if args[1] == 0x14 {
            return Ok(User32Outcome32::Return(1));
        }
        if args[1] == 0x81 {
            return Ok(User32Outcome32::Return(1));
        }
        if let Some(outcome) = self
            .window_text_message(args, wide, ram, call)
            .map_err(HostError::User32)?
        {
            return Ok(outcome);
        }
        let mut next = before;
        let result = match args[1] {
            0x30 => {
                let old = next.font;
                next.font = args[2];
                old
            }
            0x31 => before.font,
            0x401 => next.set_range(i32::from(args[3] as u16), i32::from((args[3] >> 16) as u16)),
            0x402 => next.set_position(args[2] as i32),
            0x403 => next.delta(args[2] as i32),
            0x404 => next.set_step(args[2] as i32),
            0x405 => next.advance(),
            0x406 => next.set_range(args[2] as i32, args[3] as i32),
            0x407 => {
                if args[3] != 0 {
                    let mut bytes = [0; 8];
                    bytes[..4].copy_from_slice(&before.minimum.to_le_bytes());
                    bytes[4..].copy_from_slice(&before.maximum.to_le_bytes());
                    ram.write(GuestAddress(args[3]), &bytes)
                        .map_err(E::from)
                        .map_err(HostError::User32)?;
                }
                if args[2] != 0 {
                    before.minimum as u32
                } else {
                    before.maximum as u32
                }
            }
            0x408 => before.position as u32,
            0x409 => {
                let old = next.bar;
                next.bar = args[3];
                old
            }
            0x2001 => {
                let old = next.background;
                next.background = args[3];
                old
            }
            _ => {
                return self
                    .def_window_proc(args, ram, call)
                    .map_err(HostError::User32);
            }
        };
        let dirty = next != before && !matches!(args[1], 0x30 | 0x404);
        if dirty {
            self.state.check_paint_change().map_err(HostError::User32)?;
            let extent = self
                .windows()
                .get(window)
                .map_err(E::from)
                .map_err(HostError::User32)?
                .description
                .extent;
            let region = Region32::rectangle(Rect32 {
                left: 0,
                top: 0,
                right: extent.width as i32,
                bottom: extent.height as i32,
            });
            if matches!(args[1], 0x402 | 0x403 | 0x405) && self.state.window_visible(window) {
                self.render_common_progress(
                    ProgressVisual32 {
                        window,
                        progress: next,
                        thread: call.thread_id,
                    },
                    region.clone(),
                    ram,
                    gdi.ok_or(HostError::InvalidRequest)?,
                )?;
                self.state
                    .set_update_region(window, Region32::default(), false)
                    .expect("progress change was preflighted");
            } else {
                self.state
                    .set_update_region(window, region, true)
                    .expect("progress change was preflighted");
            }
        }
        *self
            .state
            .common_state_mut(window)
            .expect("same window owns its progress state") = CommonControlState32::Progress(next);
        Ok(User32Outcome32::Return(result))
    }
}
