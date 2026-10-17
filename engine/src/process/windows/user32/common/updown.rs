use super::super::{User32CallContext32, User32Context32, User32Outcome32, memory};
use crate::{
    memory::{AddressSpace, GuestAddress},
    process::{HostError, windows::gdi::GdiRuntime32},
    windows::{
        comctl32::{CommonControlState32 as State, updown::UpDown32},
        user32::{User32Api32, User32Error32 as E},
        window::WindowHandle32,
    },
};
impl User32Context32 {
    pub(super) fn updown(&self, window: WindowHandle32) -> Result<&UpDown32, E> {
        match self.state.common_state(window)? {
            State::UpDown(value) => Ok(value),
            _ => Err(E::Win32(1407)),
        }
    }
    pub(super) fn updown_mut(&mut self, window: WindowHandle32) -> Result<&mut UpDown32, E> {
        match self.state.common_state_mut(window)? {
            State::UpDown(value) => Ok(value),
            _ => Err(E::Win32(1407)),
        }
    }
    pub(super) fn updown_buddy(&self, window: WindowHandle32) -> Result<Option<WindowHandle32>, E> {
        Ok(self
            .updown(window)?
            .buddy
            .filter(|&buddy| self.windows().get(buddy).is_ok()))
    }
    pub(super) fn updown_control_message(
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
                .updown_input(window, args, ram, call)
                .map_err(HostError::User32);
        }
        let mut redraw = false;
        let value = match args[1] {
            0x81 => {
                self.updown_mut(window).map_err(HostError::User32)?.unicode = wide;
                1
            }
            1 => {
                if self.state.record(window).map_err(HostError::User32)?.style & 0x10 != 0 {
                    let parent = self.state.record(window).map_err(HostError::User32)?.parent;
                    let buddy = parent.and_then(|parent| {
                        self.state
                            .related_windows(parent, true)
                            .take_while(|&handle| handle != window)
                            .last()
                    });
                    if let Some(buddy) = buddy {
                        return self
                            .bind_updown_buddy(window, Some(buddy), 0, ram, call)
                            .map_err(HostError::User32);
                    }
                }
                0
            }
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
                self.updown_mut(window).map_err(HostError::User32)?.font = args[2];
                redraw = args[3] != 0;
                0
            }
            0x31 => self.updown(window).map_err(HostError::User32)?.font,
            0x465 | 0x46f => {
                let (min, max) = if args[1] == 0x465 {
                    (i32::from((args[3] >> 16) as i16), i32::from(args[3] as i16))
                } else {
                    (args[2] as i32, args[3] as i32)
                };
                return self
                    .begin_updown_range(window, min, max, ram, call)
                    .map_err(HostError::User32);
            }
            0x466 => {
                let value = self.updown(window).map_err(HostError::User32)?;
                u32::from(value.maximum as u16) | (u32::from(value.minimum as u16) << 16)
            }
            0x470 => {
                let value = self.updown(window).map_err(HostError::User32)?;
                let values = [(args[2], value.minimum), (args[3], value.maximum)];
                let bytes = values.map(|(at, value)| (GuestAddress(at), value.to_le_bytes()));
                let spans = bytes
                    .iter()
                    .filter(|(at, _)| at.0 != 0)
                    .map(|(at, value)| (*at, value.as_slice()))
                    .collect::<Vec<_>>();
                ram.write_spans(&spans).map_err(HostError::Memory)?;
                0
            }
            0x467 | 0x471 => {
                let old = self.updown(window).map_err(HostError::User32)?.position;
                let pos = if args[1] == 0x467 {
                    i32::from(args[3] as i16)
                } else {
                    args[3] as i32
                };
                let result = if args[1] == 0x467 {
                    u32::from(old as u16)
                } else {
                    old as u32
                };
                return self
                    .begin_updown_set(window, pos, result, ram, call)
                    .map_err(HostError::User32);
            }
            0x468 | 0x472 => {
                return self
                    .begin_updown_read(
                        window,
                        if args[1] == 0x472 { args[3] } else { 0 },
                        args[1] == 0x468,
                        ram,
                        call,
                    )
                    .map_err(HostError::User32);
            }
            0x469 => {
                let old = self
                    .updown_buddy(window)
                    .map_err(HostError::User32)?
                    .map_or(0, |h| h.get());
                let buddy = if args[2] == 0 {
                    None
                } else {
                    Some(
                        self.owned(args[2], call.thread_id)
                            .map_err(HostError::User32)?,
                    )
                };
                return self
                    .bind_updown_buddy(window, buddy, old, ram, call)
                    .map_err(HostError::User32);
            }
            0x46a => self
                .updown_buddy(window)
                .map_err(HostError::User32)?
                .map_or(0, |h| h.get()),
            0x46b => {
                if args[2] == 0 || args[2] > 64 {
                    return Ok(User32Outcome32::Return(0));
                }
                let mut values = Vec::new();
                values
                    .try_reserve_exact(args[2] as usize)
                    .map_err(|_| HostError::User32(E::Allocation))?;
                for index in 0..args[2] {
                    let at = args[3]
                        .checked_add(index * 8)
                        .ok_or(HostError::User32(E::Win32(87)))?;
                    let [seconds, increment] =
                        memory::words::<2>(ram, at).map_err(HostError::User32)?;
                    values.push((seconds, increment));
                }
                if UpDown32::check_acceleration(&values).is_err() {
                    return Ok(User32Outcome32::Return(0));
                }
                self.updown_mut(window)
                    .map_err(HostError::User32)?
                    .acceleration = values;
                1
            }
            0x46c => {
                let value = self.updown(window).map_err(HostError::User32)?;
                if args[2] == 0 {
                    return Ok(User32Outcome32::Return(value.acceleration().len() as u32));
                }
                let count = (args[2] as usize).min(value.acceleration().len());
                let bytes = value.acceleration()[..count]
                    .iter()
                    .flat_map(|&(a, b)| [a, b].into_iter().flat_map(u32::to_le_bytes))
                    .collect::<Vec<_>>();
                ram.write(GuestAddress(args[3]), &bytes)
                    .map_err(HostError::Memory)?;
                count as u32
            }
            0x46d => {
                if !matches!(args[2], 10 | 16) {
                    return Ok(User32Outcome32::Return(0));
                }
                return self
                    .begin_updown_base(window, args[2], ram, call)
                    .map_err(HostError::User32);
            }
            0x46e => self.updown(window).map_err(HostError::User32)?.base,
            0x2005 => {
                let value = self.updown_mut(window).map_err(HostError::User32)?;
                let old = value.unicode;
                value.unicode = args[2] != 0;
                u32::from(old)
            }
            0x2006 => u32::from(self.updown(window).map_err(HostError::User32)?.unicode),
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
