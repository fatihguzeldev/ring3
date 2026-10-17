mod items;
pub(in crate::process::windows::user32) mod notifications;
mod painting;
use super::super::{User32CallContext32, User32Context32, User32Outcome32, memory};
use crate::{
    memory::{AddressSpace, GuestAddress},
    process::{HostError, windows::gdi::GdiRuntime32},
    windows::{
        comctl32::{
            CommonControlState32,
            header::{Header32, HeaderItem32, HeaderTrack32, MAX_TEXT},
        },
        gdi::Region32,
        user32::{Point32, Rect32, User32Error32 as E},
        window::WindowHandle32,
    },
};
impl User32Context32 {
    fn header(&self, window: WindowHandle32) -> Result<&Header32, E> {
        match self.state.common_state(window)? {
            CommonControlState32::Header(header) => Ok(header),
            _ => Err(E::Win32(1407)),
        }
    }
    fn header_mut(&mut self, window: WindowHandle32) -> Result<&mut Header32, E> {
        match self.state.common_state_mut(window)? {
            CommonControlState32::Header(header) => Ok(header),
            _ => Err(E::Win32(1407)),
        }
    }
    fn header_changed(&mut self, window: WindowHandle32) {
        let extent = self
            .windows()
            .get(window)
            .expect("same header owner")
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
            .expect("header paint was preflighted");
    }
    pub(super) fn header_control_message(
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
                self.header_mut(window).map_err(HostError::User32)?.unicode = wide;
                1
            }
            1 => 0,
            0x0f | 0x318 => {
                return self.paint_header(
                    window,
                    args[2],
                    args[1] == 0x318,
                    ram,
                    gdi.ok_or(HostError::InvalidRequest)?,
                    call.thread_id,
                );
            }
            0x14 => {
                return self.erase_header(
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
                self.header_mut(window).map_err(HostError::User32)?.font = args[2];
                if args[3] != 0 {
                    self.header_changed(window);
                }
                0
            }
            0x31 => self.header(window).map_err(HostError::User32)?.font,
            0x1200 => self.header(window).map_err(HostError::User32)?.items.len() as u32,
            0x1201 | 0x120a => {
                return self
                    .insert_header_item(window, index, args[3], args[1] == 0x120a, ram, call)
                    .map(User32Outcome32::Return)
                    .map_err(HostError::User32);
            }
            0x1202 => {
                if index >= self.header(window).map_err(HostError::User32)?.items.len() {
                    0
                } else {
                    self.state.check_paint_change().map_err(HostError::User32)?;
                    self.header_mut(window)
                        .map_err(HostError::User32)?
                        .delete(index);
                    self.header_changed(window);
                    1
                }
            }
            0x1203 | 0x120b => {
                return self
                    .get_header_item(window, index, args[3], args[1] == 0x120b, ram, call)
                    .map(User32Outcome32::Return)
                    .map_err(HostError::User32);
            }
            0x1204 | 0x120c => {
                if index >= self.header(window).map_err(HostError::User32)?.items.len() {
                    0
                } else {
                    let (item, order, mask) = self
                        .read_header_item(
                            window,
                            Some(index),
                            args[3],
                            args[1] == 0x120c,
                            ram,
                            call,
                        )
                        .map_err(HostError::User32)?;
                    return self
                        .begin_header_change(window, index, item, order, mask, ram, call)
                        .map_err(HostError::User32);
                }
            }
            0x1205 | 0x1206 | 0x1207 => {
                return self
                    .header_geometry(window, args, ram, gdi.ok_or(HostError::InvalidRequest)?)
                    .map(User32Outcome32::Return);
            }
            0x1208 => {
                if args[3] != 0 {
                    return Err(HostError::User32(E::Win32(50)));
                }
                0
            }
            0x1209 => 0,
            0x120f => self
                .header(window)
                .map_err(HostError::User32)?
                .order
                .get(index)
                .map_or(u32::MAX, |&value| value as u32),
            0x1211 | 0x1212 => {
                let header = self.header(window).map_err(HostError::User32)?;
                if index != header.items.len() {
                    return Ok(User32Outcome32::Return(0));
                }
                if args[1] == 0x1211 {
                    let bytes = header
                        .order
                        .iter()
                        .flat_map(|&value| (value as u32).to_le_bytes())
                        .collect::<Vec<_>>();
                    if !bytes.is_empty() {
                        ram.write(GuestAddress(args[3]), &bytes)
                            .map_err(HostError::Memory)?;
                    }
                } else {
                    let mut order = Vec::new();
                    order
                        .try_reserve_exact(index)
                        .map_err(|_| HostError::User32(E::Allocation))?;
                    for offset in 0..index {
                        let at = args[3]
                            .checked_add(offset as u32 * 4)
                            .ok_or(HostError::User32(E::Win32(87)))?;
                        order.push(
                            memory::words::<1>(ram, at).map_err(HostError::User32)?[0] as usize,
                        );
                    }
                    if header.check_order(&order).is_err() {
                        return Ok(User32Outcome32::Return(0));
                    }
                    self.state.check_paint_change().map_err(HostError::User32)?;
                    self.header_mut(window).map_err(HostError::User32)?.order = order;
                    self.header_changed(window);
                }
                1
            }
            0x1214 => {
                let value = args[2] as i32;
                if value < 0 {
                    return Err(HostError::User32(E::Win32(87)));
                }
                let header = self.header_mut(window).map_err(HostError::User32)?;
                let old = header.bitmap_margin;
                header.bitmap_margin = value;
                old as u32
            }
            0x1215 => {
                self.header(window)
                    .map_err(HostError::User32)?
                    .bitmap_margin as u32
            }
            0x2005 => {
                let header = self.header_mut(window).map_err(HostError::User32)?;
                let old = header.unicode;
                header.unicode = args[2] != 0;
                u32::from(old)
            }
            0x2006 => u32::from(self.header(window).map_err(HostError::User32)?.unicode),
            0x200 | 0x201 | 0x202 | 0x203 | 0x215 | 0x1f => {
                return self
                    .header_input(window, args, ram, call)
                    .map_err(HostError::User32);
            }
            0x1200..=0x12ff => return Err(HostError::User32(E::Win32(50))),
            _ => {
                return self
                    .prepare_arguments(
                        if wide {
                            crate::windows::user32::User32Api32::DefWindowProcW
                        } else {
                            crate::windows::user32::User32Api32::DefWindowProcA
                        },
                        args,
                        ram,
                        call,
                    )
                    .map_err(HostError::User32);
            }
        };
        Ok(User32Outcome32::Return(result))
    }
}
