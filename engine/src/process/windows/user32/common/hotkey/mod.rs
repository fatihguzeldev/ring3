mod painting;
use super::super::{User32CallContext32, User32Context32, User32Outcome32};
use crate::{
    memory::AddressSpace,
    process::{HostError, windows::gdi::GdiRuntime32},
    windows::{
        comctl32::CommonControlState32 as State,
        user32::{User32Api32, User32Error32 as E},
    },
};

impl User32Context32 {
    pub(super) fn hotkey_control_message(
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
        let State::Hotkey(before) = self.state.common_state(window).map_err(HostError::User32)?
        else {
            return Err(HostError::InvalidRequest);
        };
        let before = *before;
        if matches!(args[1], 0xf | 0x318) {
            return self.paint_hotkey(
                window,
                args[2],
                args[1] == 0x318,
                ram,
                gdi.ok_or(HostError::InvalidRequest)?,
                call.thread_id,
            );
        }
        if args[1] == 0x201 {
            if !self
                .windows()
                .get(window)
                .map_err(E::from)
                .map_err(HostError::User32)?
                .description
                .enabled
            {
                return Ok(User32Outcome32::Return(0));
            }
            return self
                .prepare_arguments(User32Api32::SetFocus, &[window.get()], ram, call)
                .map_err(HostError::User32);
        }
        let mut next = before;
        let mut notify = false;
        let value = match args[1] {
            0x81 => return Ok(User32Outcome32::Return(1)),
            0x14 => {
                return self.erase_hotkey_background(
                    window,
                    args[2],
                    ram,
                    gdi.ok_or(HostError::InvalidRequest)?,
                );
            }
            0x87 => return Ok(User32Outcome32::Return(0x81)),
            0x401 => {
                next.key = args[2] as u16;
                0
            }
            0x402 => return Ok(User32Outcome32::Return(u32::from(before.key))),
            0x403 => {
                next.invalid = args[2] as u8;
                next.defaults = args[3] as u8;
                0
            }
            0x30 => {
                next.font = args[2];
                0
            }
            0x31 => return Ok(User32Outcome32::Return(before.font)),
            0x100 | 0x104 | 0x101 | 0x105 => {
                if !self
                    .windows()
                    .get(window)
                    .map_err(E::from)
                    .map_err(HostError::User32)?
                    .description
                    .enabled
                {
                    return Ok(User32Outcome32::Return(0));
                }
                if matches!(args[1], 0x100 | 0x104) {
                    next.key_down(args[2] as u8, args[3] & (1 << 24) != 0);
                } else {
                    next.key_up(args[2] as u8);
                }
                notify = next != before;
                0
            }
            7 | 8 => {
                self.state.check_paint_change().map_err(HostError::User32)?;
                self.state
                    .invalidate_control(window)
                    .map_err(HostError::User32)?;
                return Ok(User32Outcome32::Return(0));
            }
            _ => {
                return self
                    .def_window_proc_encoded(args, wide, ram, call)
                    .map_err(HostError::User32);
            }
        };
        if next == before {
            return Ok(User32Outcome32::Return(value));
        }
        let codes: &[u32] = if notify { &[0x300] } else { &[] };
        let text = next.label(self.delivered_thread_layout(call.thread_id));
        self.check_control_transition(window, call.thread_id, None, None, codes, 2)
            .map_err(HostError::User32)?;
        let events = self.commit_control_transition(window, call.thread_id, None, None, codes);
        self.state
            .set_text(window, text)
            .expect("bounded hotkey label and checked window");
        *self.state.common_state_mut(window).unwrap() = State::Hotkey(next);
        self.state
            .invalidate_control(window)
            .expect("hotkey invalidation preflighted");
        self.control_sequence(events, value)
            .map_err(HostError::User32)
    }
}
