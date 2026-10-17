pub(in crate::process::windows::user32) mod owner;
mod painting;
use super::super::{User32CallContext32, User32Context32, User32Outcome32, memory};
use crate::{
    memory::{AddressSpace, GuestAddress},
    process::{EngineInstance, HostError, windows::gdi::GdiRuntime32},
    windows::{
        comctl32::{
            CommonControlKind32, CommonControlState32,
            status::{MAX_PARTS, MAX_TEXT, Status32},
        },
        gdi::GdiContext32,
        user32::{User32Api32, User32Error32 as E},
        window::WindowHandle32,
    },
};
use painting::StatusPaint32;

impl EngineInstance {
    pub(in crate::process::windows) fn create_status_window(
        &mut self,
        args: &[u32],
        wide: bool,
    ) -> Result<User32Outcome32, HostError> {
        let call = self.user32_call_context()?;
        if args[0] & 0x40000000 == 0 {
            return Err(HostError::User32(E::Win32(87)));
        }
        self.user32
            .as_ref()
            .ok_or(HostError::InvalidRequest)?
            .owned(args[2], call.thread_id)
            .map_err(HostError::User32)?;
        self.bind_common_control_classes(4)?;
        let atom = self
            .user32
            .as_ref()
            .unwrap()
            .state
            .common_atom(CommonControlKind32::Status)
            .ok_or(HostError::InvalidRequest)?;
        let queue = self.prepare_user32_queue_change_for(call.thread_id)?;
        let outcome = self
            .user32
            .as_mut()
            .unwrap()
            .create_window(
                if wide {
                    User32Api32::CreateWindowExW
                } else {
                    User32Api32::CreateWindowExA
                },
                &[
                    0,
                    u32::from(atom),
                    args[1],
                    args[0],
                    0,
                    0,
                    0,
                    0,
                    args[2],
                    args[3],
                    call.instance,
                    0,
                ],
                self.memory.as_mut().ok_or(HostError::Closed)?,
                call,
            )
            .map_err(HostError::User32)?;
        self.commit_user32_queue_change(queue);
        Ok(outcome)
    }
}

impl User32Context32 {
    fn status(&self, window: WindowHandle32) -> Result<&Status32, E> {
        match self.state.common_state(window)? {
            CommonControlState32::Status(status) => Ok(status),
            _ => Err(E::Win32(1407)),
        }
    }
    fn status_mut(&mut self, window: WindowHandle32) -> Result<&mut Status32, E> {
        match self.state.common_state_mut(window)? {
            CommonControlState32::Status(status) => Ok(status),
            _ => Err(E::Win32(1407)),
        }
    }
    fn status_changed(&mut self, window: WindowHandle32) {
        let extent = self
            .windows()
            .get(window)
            .expect("same status window")
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
            .expect("status paint revision was preflighted");
    }

    pub(super) fn status_control_message(
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
        let index = (args[2] & 0xff) as usize;
        let result = match args[1] {
            0x81 => {
                self.status_mut(window).map_err(HostError::User32)?.unicode = wide;
                1
            }
            1 => {
                let source = self.state.text(window).map_err(HostError::User32)?;
                let mut text = Vec::new();
                text.try_reserve_exact(source.len())
                    .map_err(|_| HostError::User32(E::Allocation))?;
                text.extend_from_slice(source);
                let outcome = self.begin_status_layout(
                    window,
                    ram,
                    gdi.ok_or(HostError::InvalidRequest)?,
                    call,
                    None,
                )?;
                self.status_mut(window)
                    .map_err(HostError::User32)?
                    .text_part_mut(0)
                    .unwrap()
                    .text = text;
                return Ok(outcome);
            }
            5 => {
                if !matches!(args[2], 0 | 2) {
                    return Ok(User32Outcome32::Return(0));
                }
                return self.begin_status_layout(
                    window,
                    ram,
                    gdi.ok_or(HostError::InvalidRequest)?,
                    call,
                    None,
                );
            }
            0x0f | 0x318 => {
                return self.paint_status(
                    StatusPaint32 {
                        window,
                        external: args[2],
                        print: args[1] == 0x318,
                        call,
                    },
                    ram,
                    gdi.ok_or(HostError::InvalidRequest)?,
                );
            }
            0x14 => 1,
            0x30 => {
                if args[3] != 0 {
                    self.state.check_paint_change().map_err(HostError::User32)?;
                }
                let outcome = self.begin_status_layout(
                    window,
                    ram,
                    gdi.ok_or(HostError::InvalidRequest)?,
                    call,
                    Some(args[2]),
                )?;
                self.status_mut(window).expect("same status owner").font = args[2];
                if args[3] != 0 {
                    self.status_changed(window);
                }
                return Ok(outcome);
            }
            0x31 => {
                let font = self.status(window).map_err(HostError::User32)?.font;
                if font == 0 {
                    GdiContext32::stock_object(17).map_err(HostError::Gdi)?
                } else {
                    font
                }
            }
            0x405 => {
                let count = args[2] as usize;
                if count == 0 || count > MAX_PARTS {
                    return Ok(User32Outcome32::Return(0));
                }
                let mut bytes = [0; MAX_PARTS * 4];
                ram.read(GuestAddress(args[3]), &mut bytes[..count * 4])
                    .map_err(E::from)
                    .map_err(HostError::User32)?;
                let mut edges = Vec::new();
                edges
                    .try_reserve_exact(count)
                    .map_err(|_| HostError::User32(E::Allocation))?;
                edges.extend(
                    bytes[..count * 4]
                        .chunks_exact(4)
                        .map(|bytes| i32::from_le_bytes(bytes.try_into().unwrap())),
                );
                let prepared = Status32::prepare_parts(edges).map_err(HostError::User32)?;
                self.state.check_paint_change().map_err(HostError::User32)?;
                self.status_mut(window)
                    .map_err(HostError::User32)?
                    .commit_parts(prepared);
                self.status_changed(window);
                1
            }
            0x406 => {
                let status = self.status(window).map_err(HostError::User32)?;
                if args[3] != 0 {
                    let count = (args[2] as usize).min(status.count());
                    let mut bytes = Vec::new();
                    bytes
                        .try_reserve_exact(count * 4)
                        .map_err(|_| HostError::User32(E::Allocation))?;
                    for position in 0..count {
                        bytes
                            .extend_from_slice(&status.part(position).unwrap().right.to_le_bytes());
                    }
                    if !bytes.is_empty() {
                        ram.write(GuestAddress(args[3]), &bytes)
                            .map_err(E::from)
                            .map_err(HostError::User32)?;
                    }
                }
                status.count() as u32
            }
            0x407 => {
                let bytes = [0, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0];
                ram.write(GuestAddress(args[3]), &bytes)
                    .map_err(E::from)
                    .map_err(HostError::User32)?;
                1
            }
            0x408 => {
                self.status_mut(window)
                    .map_err(HostError::User32)?
                    .minimum_height = (args[2] as i32).max(0);
                0
            }
            0x409 => {
                self.state.check_paint_change().map_err(HostError::User32)?;
                self.status_mut(window)
                    .map_err(HostError::User32)?
                    .simple_mode = args[2] != 0;
                self.status_changed(window);
                0
            }
            0x40a => {
                let extent = self
                    .windows()
                    .get(window)
                    .map_err(E::from)
                    .map_err(HostError::User32)?
                    .description
                    .extent;
                let Some(rect) = self.status(window).map_err(HostError::User32)?.rect(
                    args[2] as usize,
                    extent.width as i32,
                    extent.height as i32,
                ) else {
                    return Ok(User32Outcome32::Return(0));
                };
                memory::write_rect(ram, args[3], rect).map_err(HostError::User32)?;
                1
            }
            0x40e => u32::from(self.status(window).map_err(HostError::User32)?.simple_mode),
            0x2001 => {
                self.state.check_paint_change().map_err(HostError::User32)?;
                let status = self.status_mut(window).map_err(HostError::User32)?;
                let previous = status.background;
                status.background = args[3];
                self.status_changed(window);
                previous
            }
            0x2005 => {
                let status = self.status_mut(window).map_err(HostError::User32)?;
                let previous = status.unicode;
                status.unicode = args[2] != 0;
                u32::from(previous)
            }
            0x2006 => u32::from(self.status(window).map_err(HostError::User32)?.unicode),
            0x401 | 0x40b | 0x0c => {
                let part = if args[1] == 0x0c { 0 } else { index };
                let kind = if args[1] == 0x0c { 0 } else { args[2] & 0xff00 };
                if self
                    .status(window)
                    .map_err(HostError::User32)?
                    .text_part(part)
                    .is_none()
                {
                    return Ok(User32Outcome32::Return(0));
                }
                let owner = kind & 0x1000 != 0;
                if owner && part == 0xff {
                    return Ok(User32Outcome32::Return(0));
                }
                let mut text = if owner || args[3] == 0 {
                    Vec::new()
                } else {
                    memory::string(
                        ram,
                        args[3],
                        if args[1] == 0x0c {
                            wide
                        } else {
                            args[1] == 0x40b
                        },
                        MAX_TEXT,
                        call.ansi,
                    )
                    .map_err(HostError::User32)?
                };
                for unit in &mut text {
                    if *unit < 32 && *unit != 9 {
                        *unit = 32;
                    }
                }
                self.status(window)
                    .map_err(HostError::User32)?
                    .check_text(part, text.len())
                    .map_err(HostError::User32)?;
                self.state.check_paint_change().map_err(HostError::User32)?;
                let draw = if owner
                    && !self.status(window).map_err(HostError::User32)?.simple_mode
                    && self
                        .windows()
                        .get(window)
                        .map_err(E::from)
                        .map_err(HostError::User32)?
                        .description
                        .visible
                {
                    Some(
                        self.begin_status_draw(window, part, false, ram, call)
                            .map_err(HostError::User32)?,
                    )
                } else {
                    None
                };
                let target = self
                    .status_mut(window)
                    .map_err(HostError::User32)?
                    .text_part_mut(part)
                    .unwrap();
                target.text = text;
                target.kind = kind;
                target.data = if owner { args[3] } else { 0 };
                self.status_changed(window);
                return Ok(draw.unwrap_or(User32Outcome32::Return(1)));
            }
            0x402 | 0x40d | 0x403 | 0x40c | 0x0d | 0x0e => {
                let part = if matches!(args[1], 0x0d | 0x0e) {
                    0
                } else {
                    index
                };
                let Some(part) = self
                    .status(window)
                    .map_err(HostError::User32)?
                    .text_part(part)
                else {
                    return Ok(User32Outcome32::Return(0));
                };
                if part.kind & 0x1000 != 0 {
                    return Ok(User32Outcome32::Return(
                        if matches!(args[1], 0x402 | 0x40d | 0x0d) {
                            part.data
                        } else {
                            part.kind << 16
                        },
                    ));
                }
                let output_wide = if matches!(args[1], 0x0d | 0x0e) {
                    wide
                } else {
                    matches!(args[1], 0x40c | 0x40d)
                };
                let length = if output_wide {
                    part.text.len()
                } else {
                    call.ansi
                        .encode(&part.text, false)
                        .map_err(|_| HostError::User32(E::Win32(1113)))?
                        .0
                        .len()
                };
                if args[1] == 0x0d {
                    return memory::encode_text(
                        ram,
                        args[3],
                        args[2] as usize,
                        &part.text,
                        output_wide,
                        call,
                    )
                    .map(User32Outcome32::Return)
                    .map_err(HostError::User32);
                }
                if matches!(args[1], 0x402 | 0x40d) {
                    memory::encode_text(ram, args[3], length + 1, &part.text, output_wide, call)
                        .map_err(HostError::User32)?;
                }
                (length.min(0xffff) as u32)
                    | if matches!(args[1], 0x0d | 0x0e) {
                        0
                    } else {
                        part.kind << 16
                    }
            }
            0x40f => {
                let index = if args[2] == u32::MAX {
                    0xff
                } else {
                    args[2] as usize
                };
                if self
                    .status(window)
                    .map_err(HostError::User32)?
                    .text_part(index)
                    .is_none()
                {
                    return Ok(User32Outcome32::Return(0));
                }
                if args[3] != 0 {
                    let image = gdi
                        .as_ref()
                        .ok_or(HostError::InvalidRequest)?
                        .image_info(args[3])
                        .map_err(HostError::Gdi)?;
                    if image.kind != crate::windows::gdi::GdiImageKind32::Icon {
                        return Err(HostError::User32(E::Win32(6)));
                    }
                }
                self.state.check_paint_change().map_err(HostError::User32)?;
                self.status_mut(window)
                    .map_err(HostError::User32)?
                    .text_part_mut(index)
                    .unwrap()
                    .icon = args[3];
                self.status_changed(window);
                1
            }
            0x414 => {
                let index = if args[2] == u32::MAX {
                    0xff
                } else {
                    args[2] as usize
                };
                self.status(window)
                    .map_err(HostError::User32)?
                    .text_part(index)
                    .map_or(0, |part| part.icon)
            }
            0x410..=0x413 => return Err(HostError::User32(E::Win32(50))),
            _ => {
                return self
                    .def_window_proc(args, ram, call)
                    .map_err(HostError::User32);
            }
        };
        Ok(User32Outcome32::Return(result))
    }
}
