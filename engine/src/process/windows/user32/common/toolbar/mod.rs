mod input;
mod items;
mod painting;
use super::super::{User32CallContext32, User32Context32, User32Outcome32, memory};
use crate::{
    memory::{AddressSpace, GuestAddress},
    process::{HostError, windows::gdi::GdiRuntime32},
    windows::{
        comctl32::{
            CommonControlState32 as State,
            toolbar::{MAX_BUTTONS, Toolbar32, ToolbarButton32},
        },
        user32::{Point32, User32Error32 as E},
        window::WindowHandle32,
    },
};

impl User32Context32 {
    fn toolbar(&self, window: WindowHandle32) -> Result<&Toolbar32, E> {
        match self.state.common_state(window)? {
            State::Toolbar(t) => Ok(t),
            _ => Err(E::Win32(1407)),
        }
    }
    fn toolbar_mut(&mut self, window: WindowHandle32) -> Result<&mut Toolbar32, E> {
        match self.state.common_state_mut(window)? {
            State::Toolbar(t) => Ok(t),
            _ => Err(E::Win32(1407)),
        }
    }
    pub(super) fn toolbar_control_message(
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
        if matches!(args[1], 0x201 | 0x200 | 0x202 | 0x215 | 0x1f | 8 | 0xa) {
            return self
                .toolbar_input(window, args, call)
                .map_err(HostError::User32);
        }
        let result = match args[1] {
            0x81 => {
                self.toolbar_mut(window).map_err(HostError::User32)?.unicode = wide;
                1
            }
            1 => 0,
            0xf | 0x318 => {
                return self.paint_toolbar(
                    window,
                    args[2],
                    args[1] == 0x318,
                    ram,
                    gdi.ok_or(HostError::InvalidRequest)?,
                    call.thread_id,
                );
            }
            0x14 => {
                return self.erase_toolbar(
                    window,
                    args[2],
                    ram,
                    gdi.ok_or(HostError::InvalidRequest)?,
                );
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
                self.toolbar_mut(window).map_err(HostError::User32)?.font = args[2];
                if args[3] != 0 {
                    self.state
                        .invalidate_control(window)
                        .expect("toolbar font preflight");
                }
                0
            }
            0x31 => self.toolbar(window).map_err(HostError::User32)?.font,
            0x41e => {
                if args[2] != 20 {
                    return Err(HostError::User32(E::Win32(87)));
                }
                0
            }
            0x414 | 0x444 | 0x415 | 0x443 => {
                let insert = matches!(args[1], 0x415 | 0x443);
                let count = if insert { 1 } else { args[2] as usize };
                let index = if insert {
                    args[2] as usize
                } else {
                    self.toolbar(window)
                        .map_err(HostError::User32)?
                        .buttons
                        .len()
                };
                self.toolbar_insert(
                    window,
                    index,
                    count,
                    args[3],
                    matches!(args[1], 0x443 | 0x444),
                    ram,
                    call,
                )
                .map_err(HostError::User32)?
            }
            0x416 => {
                if args[2] as usize
                    >= self
                        .toolbar(window)
                        .map_err(HostError::User32)?
                        .buttons
                        .len()
                {
                    return Ok(User32Outcome32::Return(0));
                }
                self.state.check_paint_change().map_err(HostError::User32)?;
                self.toolbar_mut(window)
                    .map_err(HostError::User32)?
                    .remove(args[2] as usize);
                self.state
                    .invalidate_control(window)
                    .expect("toolbar delete preflight");
                1
            }
            0x417 => {
                let Some(b) = self
                    .toolbar(window)
                    .map_err(HostError::User32)?
                    .buttons
                    .get(args[2] as usize)
                else {
                    return Ok(User32Outcome32::Return(0));
                };
                let words = [
                    b.bitmap as u32,
                    b.command,
                    u32::from(b.state) | (u32::from(b.style) << 8),
                    b.data,
                    b.string,
                ];
                let bytes: Vec<_> = words.into_iter().flat_map(u32::to_le_bytes).collect();
                ram.write(GuestAddress(args[3]), &bytes)
                    .map_err(HostError::Memory)?;
                1
            }
            0x418 => self
                .toolbar(window)
                .map_err(HostError::User32)?
                .buttons
                .len() as u32,
            0x419 => self
                .toolbar(window)
                .map_err(HostError::User32)?
                .command(args[2])
                .map_or(u32::MAX, |i| i as u32),
            0x41c | 0x44d => {
                if args[2] != 0 {
                    return Err(HostError::User32(E::Win32(50)));
                }
                self.toolbar_add_strings(window, args[3], args[1] == 0x44d, ram, call)
                    .map_err(HostError::User32)?
            }
            0x42d | 0x44b => {
                let t = self.toolbar(window).map_err(HostError::User32)?;
                let Some(i) = t.command(args[2]) else {
                    return Ok(User32Outcome32::Return(u32::MAX));
                };
                let text = &t.buttons[i].text;
                let (bytes, count) = memory::encoded_text(
                    text.len().saturating_mul(4) + 1,
                    text,
                    args[1] == 0x44b,
                    call,
                )
                .map_err(HostError::User32)?;
                if args[3] != 0 {
                    ram.write(GuestAddress(args[3]), &bytes)
                        .map_err(HostError::Memory)?;
                }
                count
            }
            0x45b | 0x45c => {
                let t = self.toolbar(window).map_err(HostError::User32)?;
                let Some(text) = t.strings.get((args[2] >> 16) as usize) else {
                    return Ok(User32Outcome32::Return(u32::MAX));
                };
                let capacity = if args[3] == 0 {
                    text.len().saturating_mul(4) + 1
                } else {
                    args[2] as u16 as usize
                };
                let (bytes, count) = memory::encoded_text(capacity, text, args[1] == 0x45b, call)
                    .map_err(HostError::User32)?;
                if args[3] != 0 {
                    ram.write(GuestAddress(args[3]), &bytes)
                        .map_err(HostError::Memory)?;
                }
                count
            }
            0x41d | 0x433 => {
                let t = self.toolbar(window).map_err(HostError::User32)?;
                let index = if args[1] == 0x433 {
                    t.command(args[2])
                } else {
                    Some(args[2] as usize)
                };
                let Some(r) = index.and_then(|i| t.rect(i)) else {
                    return Ok(User32Outcome32::Return(0));
                };
                memory::write_rect(ram, args[3], r).map_err(HostError::User32)?;
                1
            }
            0x41f => {
                let size = ((args[3] as u16) as i32, ((args[3] >> 16) as u16) as i32);
                if size.0 == 0 || size.1 == 0 {
                    return Ok(User32Outcome32::Return(0));
                }
                self.state.check_paint_change().map_err(HostError::User32)?;
                self.toolbar_mut(window).map_err(HostError::User32)?.size = size;
                self.state
                    .invalidate_control(window)
                    .expect("toolbar size preflight");
                1
            }
            0x43a => {
                let t = self.toolbar(window).map_err(HostError::User32)?;
                t.size.0 as u32 | ((t.size.1 as u32) << 16)
            }
            0x453 => {
                let t = self.toolbar(window).map_err(HostError::User32)?;
                let width = t.layout().last().map_or(0, |(_, r)| r.right);
                let bytes: Vec<_> = [width, t.size.1]
                    .into_iter()
                    .flat_map(i32::to_le_bytes)
                    .collect();
                ram.write(GuestAddress(args[3]), &bytes)
                    .map_err(HostError::Memory)?;
                1
            }
            0x421 => return self.begin_toolbar_layout(window, ram, call),
            0x401..=0x406 | 0x409..=0x40e | 0x411 | 0x412 => {
                let Some(i) = self
                    .toolbar(window)
                    .map_err(HostError::User32)?
                    .command(args[2])
                else {
                    return Ok(User32Outcome32::Return(u32::MAX));
                };
                let old = self.toolbar(window).map_err(HostError::User32)?.buttons[i].state;
                if args[1] == 0x412 {
                    return Ok(User32Outcome32::Return(u32::from(old)));
                }
                let mask = match args[1] & 15 {
                    1 | 9 => 4,
                    2 | 10 => 1,
                    3 | 11 => 2,
                    4 | 12 => 8,
                    5 | 13 => 0x10,
                    6 | 14 => 0x80,
                    _ => 0,
                };
                if (0x409..=0x40e).contains(&args[1]) {
                    return Ok(User32Outcome32::Return(u32::from(old & mask != 0)));
                }
                let next = if args[1] == 0x411 {
                    args[3] as u8
                } else {
                    old & !mask | if args[3] as u16 != 0 { mask } else { 0 }
                };
                if next & 0x20 != 0 {
                    return Err(HostError::User32(E::Win32(50)));
                }
                self.state.check_paint_change().map_err(HostError::User32)?;
                let t = self.toolbar_mut(window).map_err(HostError::User32)?;
                if mask == 1 {
                    t.check_button(i, next & 1 != 0);
                } else {
                    t.buttons[i].state = next;
                }
                self.state
                    .invalidate_control(window)
                    .expect("toolbar state preflight");
                1
            }
            0x2005 => {
                let t = self.toolbar_mut(window).map_err(HostError::User32)?;
                let old = t.unicode;
                t.unicode = args[2] != 0;
                u32::from(old)
            }
            0x2006 => u32::from(self.toolbar(window).map_err(HostError::User32)?.unicode),
            0x400..=0x4ff => return Err(HostError::User32(E::Win32(50))),
            _ => {
                return self
                    .def_window_proc_encoded(args, wide, ram, call)
                    .map_err(HostError::User32);
            }
        };
        Ok(User32Outcome32::Return(result))
    }
}
