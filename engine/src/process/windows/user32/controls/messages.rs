use super::super::{User32CallContext32, User32Context32, User32Outcome32, memory};
use crate::{
    memory::AddressSpace,
    windows::{
        controls::ControlKind32,
        user32::User32Error32,
        window::{WindowHandle32, WindowMessage32},
    },
};
impl User32Context32 {
    pub(super) fn control_message(
        &mut self,
        args: &[u32],
        wide: bool,
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        let window = self.owned(args[0], call.thread_id)?;
        let data = self.state.record(window)?;
        let kind = self
            .state
            .standard_kind(data.class_atom)
            .ok_or(User32Error32::Win32(1407))?;
        if let Some(outcome) = self.window_text_message(args, wide, ram, call)? {
            return Ok(outcome);
        }
        if args[1] == 0x81 {
            return Ok(User32Outcome32::Return(1));
        }
        if args[1] == 0x30 {
            self.state.control(window)?.font = args[2];
            if args[3] != 0 {
                self.state.set_update_region(
                    window,
                    crate::windows::gdi::Region32::rectangle(crate::windows::user32::Rect32 {
                        left: 0,
                        top: 0,
                        right: self.windows().get(window)?.description.extent.width as i32,
                        bottom: self.windows().get(window)?.description.extent.height as i32,
                    }),
                    true,
                )?;
            }
            return Ok(User32Outcome32::Return(0));
        }
        if args[1] == 0x31 {
            return Ok(User32Outcome32::Return(self.state.control(window)?.font));
        }
        let value = match kind {
            ControlKind32::Button => match args[1] {
                0xf0 => self.state.control(window)?.check,
                0xf1 => {
                    self.state.control(window)?.check =
                        args[2].min(if matches!(data.style & 15, 5 | 6) {
                            2
                        } else {
                            1
                        });
                    0
                }
                0xf2 => {
                    let state = self.state.control(window)?;
                    state.check | if state.pressed { 4 } else { 0 }
                }
                0xf3 => {
                    self.state.control(window)?.pressed = args[2] != 0;
                    0
                }
                0xf4 => {
                    let mut data = data;
                    data.style = (data.style & !0xffff) | (args[2] & 0xffff);
                    self.state.update(data)?;
                    0
                }
                0xf5 => {
                    if !self.windows().get(window)?.description.enabled {
                        return Ok(User32Outcome32::Return(0));
                    }
                    self.check_continuation()?;
                    if matches!(data.style & 15, 3 | 6) {
                        let state = self.state.control(window)?;
                        state.check = (state.check + 1) % if data.style & 15 == 6 { 3 } else { 2 };
                    }
                    return self.control_notify(window, 0);
                }
                _ => return self.def_window_proc(args, ram, call),
            },
            ControlKind32::Edit => match args[1] {
                0xb0 => {
                    let state = self.state.control(window)?;
                    let (start, end) = state.selection;
                    let mut writes = Vec::new();
                    if args[2] != 0 {
                        writes.push(crate::memory::WordWrite32 {
                            address: crate::memory::GuestAddress(args[2]),
                            value: start as u32,
                        });
                    }
                    if args[3] != 0 {
                        writes.push(crate::memory::WordWrite32 {
                            address: crate::memory::GuestAddress(args[3]),
                            value: end as u32,
                        });
                    }
                    ram.write_words32(&writes)?;
                    if start > 65535 || end > 65535 {
                        u32::MAX
                    } else {
                        start as u32 | (end as u32) << 16
                    }
                }
                0xb1 => {
                    let length = self.state.text(window)?.len();
                    let state = self.state.control(window)?;
                    state.selection = if args[2] == u32::MAX {
                        (state.selection.1, state.selection.1)
                    } else {
                        (
                            (args[2] as usize).min(length),
                            (args[3] as usize).min(length),
                        )
                    };
                    0
                }
                0xb8 => u32::from(self.state.control(window)?.modified),
                0xb9 => {
                    self.state.control(window)?.modified = args[2] != 0;
                    0
                }
                0xc5 => {
                    self.state.control(window)?.limit = if args[2] == 0 {
                        32767
                    } else {
                        (args[2] as usize).min(1_048_576)
                    };
                    0
                }
                0xd5 => {
                    let limit = self.state.control(window)?.limit;
                    if limit == 0 { 32767 } else { limit as u32 }
                }
                0xcf => {
                    let mut data = data;
                    data.style = if args[2] != 0 {
                        data.style | 0x800
                    } else {
                        data.style & !0x800
                    };
                    self.state.update(data)?;
                    1
                }
                0xc2 => {
                    let replacement = if args[3] == 0 {
                        Vec::new()
                    } else {
                        memory::string(ram, args[3], wide, 32767, call.ansi)?
                    };
                    self.replace_edit_selection(window, &replacement)?;
                    return self.control_notify(window, 0x300);
                }
                0x102 if data.style & 0x800 == 0 => {
                    self.check_continuation()?;
                    if args[2] == 8 {
                        let length = self.state.text(window)?.len();
                        let state = self.state.control(window)?;
                        if state.selection.0 == state.selection.1 && state.selection.0 > 0 {
                            state.selection =
                                (state.selection.0 - 1, state.selection.1.min(length));
                        }
                        self.replace_edit_selection(window, &[])?;
                    } else if args[2] >= 32 || args[2] == 13 && data.style & 4 != 0 {
                        let value = if args[2] == 13 {
                            vec![13, 10]
                        } else {
                            vec![args[2] as u16]
                        };
                        self.replace_edit_selection(window, &value)?;
                    }
                    return self.control_notify(window, 0x300);
                }
                _ => return self.def_window_proc(args, ram, call),
            },
            ControlKind32::ListBox | ControlKind32::ComboBox => {
                return self.list_message(window, kind, args, wide, ram, call);
            }
            _ => return self.def_window_proc(args, ram, call),
        };
        Ok(User32Outcome32::Return(value))
    }
    fn replace_edit_selection(
        &mut self,
        window: WindowHandle32,
        replacement: &[u16],
    ) -> Result<(), User32Error32> {
        self.check_continuation()?;
        let old = self.state.text(window)?;
        let mut text = Vec::new();
        text.try_reserve_exact(old.len() + replacement.len())
            .map_err(|_| User32Error32::Allocation)?;
        text.extend_from_slice(old);
        let state = self.state.control(window)?;
        let (start, end) = (
            state.selection.0.min(state.selection.1),
            state.selection.0.max(state.selection.1),
        );
        let limit = if state.limit == 0 { 32767 } else { state.limit };
        let allowed = limit.saturating_sub(text.len() - (end - start));
        let count = replacement.len().min(allowed);
        text.splice(start..end, replacement[..count].iter().copied());
        state.selection = (start + count, start + count);
        state.modified = true;
        self.state.set_text(window, text)
    }
    fn control_notify(
        &mut self,
        window: WindowHandle32,
        code: u32,
    ) -> Result<User32Outcome32, User32Error32> {
        let data = self.state.record(window)?;
        if let Some(parent) = data.parent {
            let procedure = self.windows().get(parent)?.description.procedure;
            self.sequence(
                [
                    Some(WindowMessage32 {
                        window: parent,
                        procedure,
                        message: 0x111,
                        wparam: (data.menu & 0xffff) | (code << 16),
                        lparam: window.get(),
                    }),
                    None,
                    None,
                    None,
                    None,
                    None,
                ],
                0,
            )
        } else {
            Ok(User32Outcome32::Return(0))
        }
    }
    fn list_message(
        &mut self,
        window: WindowHandle32,
        kind: ControlKind32,
        args: &[u32],
        wide: bool,
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        let message = if kind == ControlKind32::ComboBox {
            match args[1] {
                0x143 => 0x180,
                0x14a => 0x181,
                0x144 => 0x182,
                0x14b => 0x184,
                0x148 => 0x189,
                0x14e => 0x186,
                0x147 => 0x188,
                0x146 => 0x18b,
                0x149 => 0x18a,
                0x158 => 0x199,
                0x150 => 0x19a,
                value => value,
            }
        } else {
            args[1]
        };
        let value = match message {
            0x180 | 0x181 => {
                let text = memory::string(ram, args[3], wide, 32767, call.ansi)?;
                let state = self.state.control(window)?;
                let index = if message == 0x180 || args[2] == u32::MAX {
                    state.items.len()
                } else {
                    args[2] as usize
                };
                state.insert(index, text)? as u32
            }
            0x182 => {
                let state = self.state.control(window)?;
                match state.remove(args[2] as usize) {
                    Ok(count) => count as u32,
                    Err(_) => u32::MAX,
                }
            }
            0x184 => {
                let state = self.state.control(window)?;
                state.items.clear();
                state.item_data.clear();
                state.selected = None;
                0
            }
            0x186 => {
                let state = self.state.control(window)?;
                state.selected = if args[2] as usize >= state.items.len() {
                    None
                } else {
                    Some(args[2] as usize)
                };
                state.selected.map_or(u32::MAX, |value| value as u32)
            }
            0x188 => self
                .state
                .control(window)?
                .selected
                .map_or(u32::MAX, |value| value as u32),
            0x189 | 0x18a => {
                let state = self.state.control(window)?;
                let Some(text) = state.items.get(args[2] as usize) else {
                    return Ok(User32Outcome32::Return(u32::MAX));
                };
                if message == 0x189 {
                    super::super::memory::encode_text(ram, args[3], 32768, text, wide, call)?
                } else if wide {
                    text.len() as u32
                } else {
                    call.ansi
                        .encode(text, false)
                        .map_err(|_| User32Error32::Win32(1113))?
                        .0
                        .len() as u32
                }
            }
            0x18b => self.state.control(window)?.items.len() as u32,
            0x199 => self
                .state
                .control(window)?
                .item_data
                .get(args[2] as usize)
                .copied()
                .unwrap_or(u32::MAX),
            0x19a => {
                if let Some(value) = self
                    .state
                    .control(window)?
                    .item_data
                    .get_mut(args[2] as usize)
                {
                    *value = args[3];
                    0
                } else {
                    u32::MAX
                }
            }
            _ => return self.def_window_proc(args, ram, call),
        };
        Ok(User32Outcome32::Return(value))
    }
}
