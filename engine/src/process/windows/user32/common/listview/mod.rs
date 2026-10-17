pub(in crate::process::windows::user32) mod changes;
mod columns;
mod input;
mod items;
mod layout;
mod painting;
use super::super::{User32CallContext32, User32Context32, User32Outcome32, memory};
use crate::{
    memory::{AddressSpace, GuestAddress},
    process::{HostError, windows::gdi::GdiRuntime32},
    windows::{
        comctl32::{
            CommonControlState32,
            listview::{Cell32, Item32, ListView32, MAX_ITEMS, MAX_SUBITEMS, MAX_TEXT, Patch32},
        },
        gdi::{GdiContext32, Region32},
        user32::{Point32, Rect32, User32Api32, User32Error32 as E},
        window::WindowHandle32,
    },
};
impl User32Context32 {
    fn listview(&self, window: WindowHandle32) -> Result<&ListView32, E> {
        match self.state.common_state(window)? {
            CommonControlState32::ListView(list) => Ok(list),
            _ => Err(E::Win32(1407)),
        }
    }
    fn listview_mut(&mut self, window: WindowHandle32) -> Result<&mut ListView32, E> {
        match self.state.common_state_mut(window)? {
            CommonControlState32::ListView(list) => Ok(list),
            _ => Err(E::Win32(1407)),
        }
    }
    fn listview_changed(&mut self, window: WindowHandle32) {
        let extent = self
            .windows()
            .get(window)
            .expect("same list-view owner")
            .description
            .extent;
        self.state
            .set_update_region(
                window,
                Region32::rectangle(Rect32 {
                    left: 0,
                    top: 0,
                    right: extent.width as i32,
                    bottom: extent.height as i32,
                }),
                true,
            )
            .expect("list-view paint was preflighted");
    }
    fn listview_font(&self, window: WindowHandle32) -> Result<u32, HostError> {
        let font = self.listview(window).map_err(HostError::User32)?.font;
        if font == 0 {
            GdiContext32::stock_object(17).map_err(HostError::Gdi)
        } else {
            Ok(font)
        }
    }
    pub(super) fn listview_control_message(
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
        let value = match args[1] {
            0x81 => {
                self.listview_mut(window)
                    .map_err(HostError::User32)?
                    .unicode = wide;
                1
            }
            1 => {
                return self
                    .begin_listview_header(window, ram, gdi.ok_or(HostError::InvalidRequest)?, call)
                    .map_err(HostError::User32);
            }
            5 => {
                return self
                    .layout_listview_header(
                        window,
                        ram,
                        gdi.ok_or(HostError::InvalidRequest)?,
                        call,
                    )
                    .map_err(HostError::User32);
            }
            0x0f | 0x318 => {
                return self.paint_listview(
                    window,
                    args[2],
                    args[1] == 0x318,
                    ram,
                    gdi.ok_or(HostError::InvalidRequest)?,
                    call.thread_id,
                );
            }
            0x14 => {
                return self.erase_listview(
                    window,
                    args[2],
                    ram,
                    gdi.ok_or(HostError::InvalidRequest)?,
                );
            }
            0xb => {
                if args[2] != 0 {
                    self.state.check_paint_change().map_err(HostError::User32)?;
                }
                self.listview_mut(window).map_err(HostError::User32)?.redraw = args[2] != 0;
                if args[2] != 0 {
                    self.listview_changed(window)
                }
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
                let header = self.listview(window).map_err(HostError::User32)?.header;
                self.listview_mut(window).map_err(HostError::User32)?.font = args[2];
                if let Some(header) = header {
                    if let Ok(CommonControlState32::Header(state)) =
                        self.state.common_state_mut(header)
                    {
                        state.font = args[2];
                    }
                }
                if args[3] != 0 {
                    self.listview_changed(window)
                }
                0
            }
            0x31 => self.listview(window).map_err(HostError::User32)?.font,
            0x87 => 0x81,
            0x1004 => self
                .listview(window)
                .map_err(HostError::User32)?
                .items
                .len() as u32,
            0x1005 | 0x104b | 0x102d | 0x1073 => {
                return self
                    .get_listview_item(window, args, ram, call)
                    .map(User32Outcome32::Return)
                    .map_err(HostError::User32);
            }
            0x1006 | 0x104c | 0x1007 | 0x104d | 0x102e | 0x1074 | 0x102b => {
                return self
                    .set_listview_item(window, args, ram, call)
                    .map_err(HostError::User32);
            }
            0x1008 | 0x1009 => {
                return self
                    .delete_listview_items(window, args[2], args[1] == 0x1009, ram, call)
                    .map_err(HostError::User32);
            }
            0x102c => self
                .listview(window)
                .map_err(HostError::User32)?
                .items
                .get(args[2] as usize)
                .map_or(0, |item| item.state & args[3]),
            0x1032 => self
                .listview(window)
                .map_err(HostError::User32)?
                .items
                .iter()
                .filter(|item| item.state & 2 != 0)
                .count() as u32,
            0x100c => {
                if args[3] & !15 != 0 {
                    return Err(HostError::User32(E::Win32(50)));
                }
                let list = self.listview(window).map_err(HostError::User32)?;
                let start = (args[2] as i32).saturating_add(1).max(0) as usize;
                list.items
                    .iter()
                    .enumerate()
                    .skip(start)
                    .find(|(_, item)| item.state & args[3] == args[3])
                    .map_or(u32::MAX, |(index, _)| index as u32)
            }
            0x100d | 0x1053 => {
                return self
                    .find_listview_item(window, args, ram, call)
                    .map(User32Outcome32::Return)
                    .map_err(HostError::User32);
            }
            0x1019 | 0x105f | 0x101a | 0x1060 | 0x101b | 0x1061 | 0x101c | 0x101d | 0x101e
            | 0x101f | 0x103a | 0x103b => {
                return self.listview_column_message(
                    window,
                    args,
                    ram,
                    call,
                    gdi.ok_or(HostError::InvalidRequest)?,
                );
            }
            0x1000 | 0x1023 | 0x1025 => {
                let list = self.listview(window).map_err(HostError::User32)?;
                let (value, color) = match args[1] {
                    0x1000 => (list.background, 5),
                    0x1023 => (list.text_color, 8),
                    _ => (list.text_background, 5),
                };
                value.unwrap_or(gdi.ok_or(HostError::InvalidRequest)?.system_color(color))
            }
            0x1001 | 0x1024 | 0x1026 => {
                if args[1] == 0x1001 && args[3] == u32::MAX {
                    return Err(HostError::User32(E::Win32(50)));
                }
                self.state.check_paint_change().map_err(HostError::User32)?;
                let list = self.listview_mut(window).map_err(HostError::User32)?;
                let value = Some(args[3]);
                match args[1] {
                    0x1001 => list.background = value,
                    0x1024 => list.text_color = value,
                    _ => list.text_background = value,
                }
                self.listview_changed(window);
                1
            }
            0x1002 | 0x100a | 0x1018 => 0,
            0x1003 | 0x100b => {
                if args[3] != 0 {
                    return Err(HostError::User32(E::Win32(50)));
                }
                0
            }
            0x102f => {
                if args[2] as usize > MAX_ITEMS {
                    return Err(HostError::User32(E::Capacity));
                }
                {
                    let reserve = (args[2] as usize).saturating_sub(
                        self.listview(window)
                            .map_err(HostError::User32)?
                            .items
                            .len(),
                    );
                    self.listview_mut(window)
                        .map_err(HostError::User32)?
                        .items
                        .try_reserve(reserve)
                }
                .map_err(|_| HostError::User32(E::Allocation))?;
                1
            }
            0x1036 => {
                let mask = if args[2] == 0 { u32::MAX } else { args[2] };
                if args[3] & mask & !0x10021 != 0 {
                    return Err(HostError::User32(E::Win32(50)));
                }
                self.state.check_paint_change().map_err(HostError::User32)?;
                let list = self.listview_mut(window).map_err(HostError::User32)?;
                let old = list.extended;
                list.extended = (old & !mask) | (args[3] & mask);
                self.listview_changed(window);
                old
            }
            0x1037 => self.listview(window).map_err(HostError::User32)?.extended,
            0x2005 => {
                let list = self.listview_mut(window).map_err(HostError::User32)?;
                let old = list.unicode;
                list.unicode = args[2] != 0;
                u32::from(old)
            }
            0x2006 => u32::from(self.listview(window).map_err(HostError::User32)?.unicode),
            0x100e | 0x1010 | 0x1012 | 0x1013 | 0x1014 | 0x1015 | 0x1027 | 0x1028 | 0x1038 => {
                return self
                    .listview_geometry_message(
                        window,
                        args,
                        ram,
                        gdi.ok_or(HostError::InvalidRequest)?,
                    )
                    .map(User32Outcome32::Return);
            }
            0x7 | 0x8 => {
                self.state.check_paint_change().map_err(HostError::User32)?;
                let notice = self
                    .prepare_listview_notice(
                        window,
                        if args[1] == 7 { -7 } else { -8 },
                        -1,
                        0,
                        0,
                        0,
                        0,
                        0,
                        Point32::default(),
                        0,
                        ram,
                        call,
                    )
                    .map_err(HostError::User32)?;
                self.listview_changed(window);
                return self
                    .publish_listview_call(notice)
                    .map_err(HostError::User32);
            }
            0x4e => {
                let header = self.listview(window).map_err(HostError::User32)?.header;
                if header.map(WindowHandle32::get)
                    == Some(memory::words::<1>(ram, args[3]).map_err(HostError::User32)?[0])
                {
                    self.state.check_paint_change().map_err(HostError::User32)?;
                    let parent = self.state.record(window).map_err(HostError::User32)?.parent;
                    if let Some(parent) = parent {
                        let outcome = self
                            .send_message(&[parent.get(), 0x4e, args[2], args[3]], call)
                            .map_err(HostError::User32)?;
                        self.listview_changed(window);
                        return Ok(outcome);
                    }
                    self.listview_changed(window);
                }
                0
            }
            0x100 | 0x201 | 0x202 | 0x203 | 0x204 | 0x205 => {
                return self.listview_input(
                    window,
                    args,
                    ram,
                    gdi.ok_or(HostError::InvalidRequest)?,
                    call,
                );
            }
            0x1000..=0x10ff => return Err(HostError::User32(E::Win32(50))),
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
        Ok(User32Outcome32::Return(value))
    }
}
