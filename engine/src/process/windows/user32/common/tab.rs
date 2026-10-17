mod painting;
use super::super::{User32CallContext32, User32Context32, User32Outcome32, memory};
use crate::{
    memory::{AddressSpace, GuestAddress},
    process::{HostError, windows::gdi::GdiRuntime32},
    windows::{
        comctl32::{
            CommonControlState32,
            tab::{MAX_TEXT, Tab32, TabItem32},
        },
        user32::User32Error32 as E,
        window::WindowHandle32,
    },
};

impl User32Context32 {
    fn tab(&self, window: WindowHandle32) -> Result<&Tab32, E> {
        match self.state.common_state(window)? {
            CommonControlState32::Tab(tab) => Ok(tab),
            _ => Err(E::Win32(1407)),
        }
    }
    fn tab_mut(&mut self, window: WindowHandle32) -> Result<&mut Tab32, E> {
        match self.state.common_state_mut(window)? {
            CommonControlState32::Tab(tab) => Ok(tab),
            _ => Err(E::Win32(1407)),
        }
    }
    fn tab_changed(&mut self, window: WindowHandle32) {
        let extent = self
            .windows()
            .get(window)
            .expect("same tab owner")
            .description
            .extent;
        self.state
            .set_update_region(
                window,
                crate::windows::gdi::Region32::rectangle(crate::windows::user32::Rect32 {
                    left: 0,
                    top: 0,
                    right: extent.width as i32,
                    bottom: extent.height as i32,
                }),
                true,
            )
            .expect("tab paint was preflighted");
    }
    pub(super) fn tab_control_message(
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
        let index = args[2] as usize;
        let result = match args[1] {
            0x81 => {
                self.tab_mut(window).map_err(HostError::User32)?.unicode = wide;
                1
            }
            1 => 0,
            0x0f | 0x318 => {
                return self.paint_tab(
                    window,
                    args[2],
                    args[1] == 0x318,
                    ram,
                    gdi.ok_or(HostError::InvalidRequest)?,
                    call.thread_id,
                );
            }
            0x14 => 1,
            0x87 => 0x81,
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
                self.tab_mut(window).map_err(HostError::User32)?.font = args[2];
                if args[3] != 0 {
                    self.tab_changed(window);
                }
                0
            }
            0x31 => self.tab(window).map_err(HostError::User32)?.font,
            0x1302 => 0,
            0x1303 => {
                if args[3] != 0 {
                    return Err(HostError::User32(E::Win32(50)));
                }
                0
            }
            0x1304 => self.tab(window).map_err(HostError::User32)?.items.len() as u32,
            0x1305 | 0x133c => {
                return self
                    .get_tab_item(window, index, args[3], args[1] == 0x133c, ram, call)
                    .map(User32Outcome32::Return)
                    .map_err(HostError::User32);
            }
            0x1306 | 0x133d | 0x1307 | 0x133e => {
                return self
                    .set_tab_item(
                        window,
                        index,
                        args[3],
                        matches!(args[1], 0x133d | 0x133e),
                        matches!(args[1], 0x1307 | 0x133e),
                        ram,
                        call,
                    )
                    .map(User32Outcome32::Return)
                    .map_err(HostError::User32);
            }
            0x1308 => {
                if index >= self.tab(window).map_err(HostError::User32)?.items.len() {
                    0
                } else {
                    self.state.check_paint_change().map_err(HostError::User32)?;
                    self.tab_mut(window)
                        .map_err(HostError::User32)?
                        .delete(index);
                    self.tab_changed(window);
                    1
                }
            }
            0x1309 => {
                self.state.check_paint_change().map_err(HostError::User32)?;
                let tab = self.tab_mut(window).map_err(HostError::User32)?;
                tab.items.clear();
                tab.selected = -1;
                tab.focused = -1;
                self.tab_changed(window);
                1
            }
            0x130a | 0x130d | 0x1328 | 0x132c => {
                return self
                    .tab_geometry_message(
                        window,
                        args,
                        ram,
                        gdi.ok_or(HostError::InvalidRequest)?,
                        call.thread_id,
                    )
                    .map(User32Outcome32::Return);
            }
            0x130b => self.tab(window).map_err(HostError::User32)?.selected as u32,
            0x130c => {
                let valid = (args[2] as i32) >= -1
                    && (args[2] as i32)
                        < self.tab(window).map_err(HostError::User32)?.items.len() as i32;
                if !valid {
                    u32::MAX
                } else {
                    self.state.check_paint_change().map_err(HostError::User32)?;
                    let old = self
                        .tab_mut(window)
                        .map_err(HostError::User32)?
                        .select(args[2] as i32);
                    self.tab_changed(window);
                    old as u32
                }
            }
            0x130e => {
                let tab = self.tab_mut(window).map_err(HostError::User32)?;
                if !tab.items.is_empty() || args[2] > 4096 {
                    0
                } else {
                    tab.extra = args[2] as usize;
                    1
                }
            }
            0x1329 => {
                let width = i32::from(args[3] as u16);
                let height = i32::from((args[3] >> 16) as u16);
                if width == 0 || height == 0 {
                    return Err(HostError::User32(E::Win32(87)));
                }
                self.state.check_paint_change().map_err(HostError::User32)?;
                let tab = self.tab_mut(window).map_err(HostError::User32)?;
                let old = tab.item_size.unwrap_or((0, 0));
                tab.item_size = Some((width, height));
                self.tab_changed(window);
                old.0 as u32 | ((old.1 as u32) << 16)
            }
            0x132b => {
                self.state.check_paint_change().map_err(HostError::User32)?;
                self.tab_mut(window).map_err(HostError::User32)?.padding =
                    (i32::from(args[3] as u16), i32::from((args[3] >> 16) as u16));
                self.tab_changed(window);
                0
            }
            0x132d => 0,
            0x132e => {
                if args[2] != 0 {
                    return Err(HostError::User32(E::Win32(50)));
                }
                0
            }
            0x132f => self.tab(window).map_err(HostError::User32)?.focused as u32,
            0x1330 => {
                if index < self.tab(window).map_err(HostError::User32)?.items.len() {
                    self.state.check_paint_change().map_err(HostError::User32)?;
                    self.tab_mut(window).map_err(HostError::User32)?.focused = index as i32;
                    self.tab_changed(window);
                }
                0
            }
            0x1331 => {
                let tab = self.tab_mut(window).map_err(HostError::User32)?;
                let old = tab.min_width;
                tab.min_width = args[3] as i32;
                old as u32
            }
            0x1332 => {
                self.state.check_paint_change().map_err(HostError::User32)?;
                let tab = self.tab_mut(window).map_err(HostError::User32)?;
                for (index, item) in tab.items.iter_mut().enumerate() {
                    if args[2] == 0 || index as i32 != tab.selected {
                        item.state &= !1;
                    }
                }
                self.tab_changed(window);
                0
            }
            0x1333 => {
                let Some(item) = self
                    .tab(window)
                    .map_err(HostError::User32)?
                    .items
                    .get(index)
                else {
                    return Ok(User32Outcome32::Return(0));
                };
                let next = if args[3] & 0xffff != 0 {
                    item.state | 2
                } else {
                    item.state & !2
                };
                self.state.check_paint_change().map_err(HostError::User32)?;
                self.tab_mut(window).map_err(HostError::User32)?.items[index].state = next;
                self.tab_changed(window);
                1
            }
            0x1334 => {
                let tab = self.tab_mut(window).map_err(HostError::User32)?;
                let old = tab.extended;
                let mask = if args[2] == 0 { u32::MAX } else { args[2] };
                tab.extended = (old & !mask) | (args[3] & mask);
                old
            }
            0x1335 => self.tab(window).map_err(HostError::User32)?.extended,
            0x2005 => {
                let tab = self.tab_mut(window).map_err(HostError::User32)?;
                let old = tab.unicode;
                tab.unicode = args[2] != 0;
                u32::from(old)
            }
            0x2006 => u32::from(self.tab(window).map_err(HostError::User32)?.unicode),
            0x100 | 0x201 | 0x202 => return Err(HostError::User32(E::Win32(50))),
            _ => {
                return self
                    .def_window_proc(args, ram, call)
                    .map_err(HostError::User32);
            }
        };
        Ok(User32Outcome32::Return(result))
    }
    fn get_tab_item(
        &self,
        window: WindowHandle32,
        index: usize,
        pointer: u32,
        wide: bool,
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<u32, E> {
        let Some(item) = self.tab(window)?.items.get(index) else {
            return Ok(0);
        };
        let mut words = memory::words::<6>(ram, pointer)?;
        let mask = words[0];
        let mut output = Vec::new();
        output
            .try_reserve_exact(24 + self.tab(window)?.extra)
            .map_err(|_| E::Allocation)?;
        if mask & 0x10 != 0 {
            words[1] = item.state & words[2];
        }
        if mask & 2 != 0 {
            words[5] = item.image as u32;
        }
        for word in words {
            output.extend_from_slice(&word.to_le_bytes());
        }
        if mask & 8 != 0 {
            output.extend_from_slice(&item.data);
        }
        let text = if mask & 1 != 0 && words[4] != 0 {
            memory::encoded_text(words[4] as usize, &item.text, wide, call)?.0
        } else {
            Vec::new()
        };
        ram.write_spans(&[
            (GuestAddress(pointer), &output),
            (GuestAddress(words[3]), &text),
        ])?;
        Ok(1)
    }
    fn set_tab_item(
        &mut self,
        window: WindowHandle32,
        index: usize,
        pointer: u32,
        wide: bool,
        insert: bool,
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<u32, E> {
        let words = memory::words::<6>(ram, pointer)?;
        let mask = words[0];
        let tab = self.tab(window)?;
        if !insert && index >= tab.items.len() {
            return Ok(0);
        }
        let old = (!insert).then(|| &tab.items[index]);
        let mut text = Vec::new();
        if mask & 1 != 0 {
            if words[3] != 0 {
                text = memory::string(ram, words[3], wide, MAX_TEXT, call.ansi)?;
            }
        } else if let Some(old) = old {
            text.try_reserve_exact(old.text.len())
                .map_err(|_| E::Allocation)?;
            text.extend_from_slice(&old.text);
        }
        let mut data = Vec::new();
        data.try_reserve_exact(tab.extra)
            .map_err(|_| E::Allocation)?;
        data.resize(tab.extra, 0);
        if mask & 8 != 0 {
            ram.read(
                GuestAddress(pointer.checked_add(24).ok_or(E::Win32(87))?),
                &mut data,
            )?;
        } else if let Some(old) = old {
            data.copy_from_slice(&old.data);
        }
        let item = TabItem32 {
            text,
            image: if mask & 2 != 0 {
                words[5] as i32
            } else {
                old.map_or(-1, |item| item.image)
            },
            state: if insert {
                0
            } else if mask & 0x10 != 0 {
                (old.unwrap().state & !words[2]) | (words[1] & words[2])
            } else {
                old.unwrap().state
            },
            data,
        };
        self.tab(window)?
            .check_item((!insert).then_some(index), &item)?;
        self.state.check_paint_change()?;
        if insert {
            self.tab_mut(window)?.prepare_insert(&item)?;
            let index = self.tab_mut(window)?.insert(index, item);
            self.tab_changed(window);
            Ok(index as u32)
        } else {
            self.tab_mut(window)?.items[index] = item;
            self.tab_changed(window);
            Ok(1)
        }
    }
}
