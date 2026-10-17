mod painting;
pub(in crate::process::windows::user32) mod relay;
pub(in crate::process::windows::user32) mod text;
pub(in crate::process::windows::user32) mod tracking;
use super::super::{User32CallContext32, User32Context32, User32Outcome32, memory};
use crate::{
    memory::{AddressSpace, GuestAddress},
    process::{HostError, windows::gdi::GdiRuntime32},
    windows::{
        comctl32::{
            CommonControlState32,
            tooltip::{MAX_TEXT, Tool32, Tooltip32},
        },
        user32::{Point32, Rect32, User32Error32 as E},
        window::WindowHandle32,
    },
};
#[derive(Clone, Copy)]
enum TrackingChange32<'a> {
    Text(&'a Tool32),
    Position(Point32),
    Resolved(Point32),
}
struct ToolInfo32 {
    size: usize,
    words: [u32; 12],
}
fn tool_key(ram: &AddressSpace, pointer: u32) -> Result<(usize, u32, u32), E> {
    let words = memory::words::<4>(ram, pointer)?;
    if !matches!(words[0], 40 | 44 | 48) {
        return Err(E::Win32(87));
    }
    Ok((words[0] as usize, words[2], words[3]))
}
fn tool_info(ram: &AddressSpace, pointer: u32) -> Result<ToolInfo32, E> {
    let (size, _, _) = tool_key(ram, pointer)?;
    let mut bytes = [0; 48];
    ram.read(GuestAddress(pointer), &mut bytes[..size])?;
    Ok(ToolInfo32 {
        size,
        words: std::array::from_fn(|index| {
            u32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap())
        }),
    })
}
impl User32Context32 {
    fn tooltip(&self, window: WindowHandle32) -> Result<&Tooltip32, E> {
        match self.state.common_state(window)? {
            CommonControlState32::Tooltip(tip) => Ok(tip),
            _ => Err(E::Win32(1407)),
        }
    }
    fn tooltip_mut(&mut self, window: WindowHandle32) -> Result<&mut Tooltip32, E> {
        match self.state.common_state_mut(window)? {
            CommonControlState32::Tooltip(tip) => Ok(tip),
            _ => Err(E::Win32(1407)),
        }
    }
    pub(super) fn tooltip_control_message(
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
        let result = match args[1] {
            0x81 => {
                self.tooltip_mut(window).map_err(HostError::User32)?.unicode = wide;
                1
            }
            1 => 0,
            0x0f => {
                return self.paint_tooltip(
                    window,
                    ram,
                    gdi.ok_or(HostError::InvalidRequest)?,
                    call.thread_id,
                );
            }
            0x14 => {
                if args[2] == 0 {
                    return Ok(User32Outcome32::Return(0));
                }
                let gdi = gdi.ok_or(HostError::InvalidRequest)?;
                let tip = self.tooltip(window).map_err(HostError::User32)?;
                let color = if tip.background == 0xff000000 {
                    gdi.system_color(24)
                } else {
                    tip.background
                };
                let extent = self
                    .windows()
                    .get(window)
                    .map_err(E::from)
                    .map_err(HostError::User32)?
                    .description
                    .extent;
                let mut canvas = gdi.begin_canvas(ram, args[2]).map_err(HostError::Gdi)?;
                canvas.fill(
                    gdi,
                    Rect32 {
                        left: 0,
                        top: 0,
                        right: extent.width as i32,
                        bottom: extent.height as i32,
                    },
                    color,
                );
                gdi.publish_canvas(ram, canvas).map_err(HostError::Gdi)?;
                1
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
                self.tooltip_mut(window).map_err(HostError::User32)?.font = args[2];
                if args[3] != 0 {
                    self.tooltip_changed(window);
                }
                0
            }
            0x31 => self.tooltip(window).map_err(HostError::User32)?.font,
            0x401 => {
                let active = self.tooltip(window).map_err(HostError::User32)?.active;
                let outcome = if args[2] == 0 && active.is_some() {
                    self.begin_tooltip_tracking(
                        window,
                        None,
                        ram,
                        gdi.ok_or(HostError::InvalidRequest)?,
                        call,
                    )?
                } else {
                    User32Outcome32::Return(0)
                };
                if args[2] == 0 {
                    self.timers.kill(call.thread_id, window.get(), 1);
                }
                let tip = self.tooltip_mut(window).expect("same tooltip");
                tip.enabled = args[2] != 0;
                if !tip.enabled {
                    tip.hover = None;
                    tip.automatic = false;
                    tip.suppressed = false;
                }
                return Ok(outcome);
            }
            0x404 | 0x432 | 0x409 | 0x436 | 0x40c | 0x439 => {
                let Some((index, tool, adding)) = self
                    .prepare_tool(window, args, ram, call)
                    .map_err(HostError::User32)?
                else {
                    return Ok(User32Outcome32::Return(0));
                };
                let outcome = if index.is_some()
                    && self.tooltip(window).map_err(HostError::User32)?.active == Some(tool.key)
                {
                    self.begin_tooltip_tracking_change(
                        window,
                        Some(tool.key),
                        ram,
                        gdi.ok_or(HostError::InvalidRequest)?,
                        call,
                        Some(TrackingChange32::Text(&tool)),
                    )?
                } else {
                    User32Outcome32::Return(u32::from(adding))
                };
                if let Some(index) = index {
                    self.tooltip_mut(window).expect("same tooltip").tools[index] = tool;
                } else {
                    self.tooltip_mut(window).expect("same tooltip").insert(tool);
                }
                return Ok(outcome);
            }
            0x405 | 0x433 => {
                let (_, owner, id) = tool_key(ram, args[3]).map_err(HostError::User32)?;
                let Some(index) = self
                    .tooltip(window)
                    .map_err(HostError::User32)?
                    .find(owner, id)
                else {
                    return Ok(User32Outcome32::Return(0));
                };
                if self.tooltip(window).map_err(HostError::User32)?.active
                    == Some(self.tooltip(window).map_err(HostError::User32)?.tools[index].key)
                {
                    let outcome = self.begin_tooltip_tracking(
                        window,
                        None,
                        ram,
                        gdi.ok_or(HostError::InvalidRequest)?,
                        call,
                    )?;
                    if self.tooltip(window).expect("same tooltip").hover
                        == Some(self.tooltip(window).expect("same tooltip").tools[index].key)
                    {
                        self.timers.kill(call.thread_id, window.get(), 1);
                    }
                    self.tooltip_mut(window)
                        .expect("same tooltip")
                        .remove(index);
                    return Ok(outcome);
                }
                if self.tooltip(window).expect("same tooltip").hover
                    == Some(self.tooltip(window).expect("same tooltip").tools[index].key)
                {
                    self.timers.kill(call.thread_id, window.get(), 1);
                }
                self.tooltip_mut(window)
                    .expect("same tooltip")
                    .remove(index);
                0
            }
            0x408 | 0x435 | 0x40b | 0x438 | 0x40e | 0x43a | 0x40f | 0x43b => {
                return self
                    .get_tool(window, args, ram, call)
                    .map(User32Outcome32::Return)
                    .map_err(HostError::User32);
            }
            0x40d => self.tooltip(window).map_err(HostError::User32)?.tools.len() as u32,
            0x411 => {
                let (_, owner, id) = tool_key(ram, args[3]).map_err(HostError::User32)?;
                let tip = self.tooltip(window).map_err(HostError::User32)?;
                let target = if args[2] != 0 {
                    let Some(index) = tip.find(owner, id) else {
                        return Ok(User32Outcome32::Return(0));
                    };
                    if tip.tools[index].flags & 0x20 == 0 {
                        return Err(HostError::User32(E::Win32(87)));
                    }
                    Some(tip.tools[index].key)
                } else {
                    None
                };
                let outcome = self.begin_tooltip_tracking(
                    window,
                    target,
                    ram,
                    gdi.ok_or(HostError::InvalidRequest)?,
                    call,
                )?;
                self.timers.kill(call.thread_id, window.get(), 1);
                let tip = self.tooltip_mut(window).expect("same tooltip");
                tip.hover = None;
                tip.automatic = false;
                tip.suppressed = false;
                return Ok(outcome);
            }
            0x412 => {
                let point = Point32 {
                    x: i32::from(args[3] as i16),
                    y: i32::from((args[3] >> 16) as i16),
                };
                if let Some(key) = self.tooltip(window).map_err(HostError::User32)?.active {
                    return self.begin_tooltip_tracking_change(
                        window,
                        Some(key),
                        ram,
                        gdi.ok_or(HostError::InvalidRequest)?,
                        call,
                        Some(TrackingChange32::Position(point)),
                    );
                }
                self.tooltip_mut(window).expect("same tooltip").point = point;
                0
            }
            0x41d => {
                let key = self.tooltip(window).map_err(HostError::User32)?.active;
                return self.begin_tooltip_tracking(
                    window,
                    key,
                    ram,
                    gdi.ok_or(HostError::InvalidRequest)?,
                    call,
                );
            }
            0x407 => {
                let message = memory::message(ram, args[3]).map_err(HostError::User32)?;
                return self.relay_tooltip_event(
                    window,
                    message,
                    ram,
                    gdi.ok_or(HostError::InvalidRequest)?,
                    call,
                );
            }
            0x113 => {
                return self.tooltip_timer(
                    window,
                    args[2],
                    ram,
                    gdi.ok_or(HostError::InvalidRequest)?,
                    call,
                );
            }
            0x41c => {
                return self.begin_tooltip_tracking(
                    window,
                    None,
                    ram,
                    gdi.ok_or(HostError::InvalidRequest)?,
                    call,
                );
            }
            0x413 | 0x414 => {
                self.state.check_paint_change().map_err(HostError::User32)?;
                let tip = self.tooltip_mut(window).map_err(HostError::User32)?;
                if args[1] == 0x413 {
                    tip.background = args[2];
                } else {
                    tip.foreground = args[2];
                }
                self.tooltip_changed(window);
                0
            }
            0x416 => {
                let color = self.tooltip(window).map_err(HostError::User32)?.background;
                if color == 0xff000000 {
                    gdi.as_ref()
                        .ok_or(HostError::InvalidRequest)?
                        .system_color(24)
                } else {
                    color
                }
            }
            0x417 => {
                let color = self.tooltip(window).map_err(HostError::User32)?.foreground;
                if color == 0xff000000 {
                    gdi.as_ref()
                        .ok_or(HostError::InvalidRequest)?
                        .system_color(23)
                } else {
                    color
                }
            }
            0x419 => u32::MAX,
            0x418 => {
                if args[3] as i32 >= 0 {
                    return Err(HostError::User32(E::Win32(50)));
                }
                u32::MAX
            }
            0x41a => {
                let rect = memory::rect(ram, args[3]).map_err(HostError::User32)?;
                if [rect.left, rect.top, rect.right, rect.bottom]
                    .iter()
                    .any(|&value| !(0..=1024).contains(&value))
                {
                    return Err(HostError::User32(E::Win32(87)));
                }
                self.tooltip_mut(window).map_err(HostError::User32)?.margin = rect;
                0
            }
            0x41b => {
                memory::write_rect(
                    ram,
                    args[3],
                    self.tooltip(window).map_err(HostError::User32)?.margin,
                )
                .map_err(HostError::User32)?;
                0
            }
            0x403 => {
                let requested = args[3] as i16;
                let delay = if requested < 0 { 500 } else { requested as u32 };
                let tip = self.tooltip_mut(window).map_err(HostError::User32)?;
                match args[2] {
                    0 => tip.delays = [delay, delay.saturating_mul(10), delay / 5],
                    1 => tip.delays[2] = if requested < 0 { 100 } else { delay },
                    2 => tip.delays[1] = if requested < 0 { 5000 } else { delay },
                    3 => tip.delays[0] = delay,
                    _ => return Err(HostError::User32(E::Win32(87))),
                }
                0
            }
            0x415 => {
                let tip = self.tooltip(window).map_err(HostError::User32)?;
                match args[2] {
                    1 => tip.delays[2],
                    2 => tip.delays[1],
                    3 => tip.delays[0],
                    _ => return Err(HostError::User32(E::Win32(87))),
                }
            }
            0x2005 => {
                let tip = self.tooltip_mut(window).map_err(HostError::User32)?;
                let old = tip.unicode;
                tip.unicode = args[2] != 0;
                u32::from(old)
            }
            0x2006 => u32::from(self.tooltip(window).map_err(HostError::User32)?.unicode),
            _ => {
                return self
                    .def_window_proc(args, ram, call)
                    .map_err(HostError::User32);
            }
        };
        Ok(User32Outcome32::Return(result))
    }
    fn tooltip_changed(&mut self, window: WindowHandle32) {
        let extent = self
            .windows()
            .get(window)
            .expect("same tooltip")
            .description
            .extent;
        self.state
            .set_update_region(
                window,
                crate::windows::gdi::Region32::rectangle(Rect32 {
                    left: 0,
                    top: 0,
                    right: extent.width as i32,
                    bottom: extent.height as i32,
                }),
                true,
            )
            .expect("tooltip paint was preflighted");
    }
    fn prepare_tool(
        &mut self,
        window: WindowHandle32,
        args: &[u32],
        ram: &AddressSpace,
        call: User32CallContext32,
    ) -> Result<Option<(Option<usize>, Tool32, bool)>, E> {
        let info = tool_info(ram, args[3])?;
        let mut words = info.words;
        let adding = matches!(args[1], 0x404 | 0x432);
        let index = self.tooltip(window)?.find(words[2], words[3]);
        if adding && index.is_some() {
            return Ok(None);
        }
        if !adding && index.is_none() {
            return Ok(None);
        }
        if matches!(args[1], 0x40c | 0x439) {
            let old = &self.tooltip(window)?.tools[index.unwrap()];
            words[1] = old.flags;
            words[4] = old.rect.left as u32;
            words[5] = old.rect.top as u32;
            words[6] = old.rect.right as u32;
            words[7] = old.rect.bottom as u32;
            words[10] = old.data;
        }
        if info.size == 48 && words[11] != 0 {
            return Err(E::Win32(87));
        }
        if words[1] & !0xb7 != 0 || words[8] != 0 {
            return Err(E::Win32(50));
        }
        if words[1] & 0x20 != 0 && words[1] & 0x80 == 0 {
            return Err(E::Win32(50));
        }
        if words[2] != 0 {
            self.owned(words[2], call.thread_id)?;
        }
        if words[1] & 1 != 0 {
            self.owned(words[3], call.thread_id)?;
        }
        let text = if matches!(words[9], 0 | u32::MAX) {
            Vec::new()
        } else {
            memory::string(
                ram,
                words[9],
                matches!(args[1], 0x432 | 0x436 | 0x439),
                MAX_TEXT,
                call.ansi,
            )?
        };
        let tool = Tool32 {
            revision: index.map_or(Ok(1), |index| {
                self.tooltip(window).unwrap().tools[index]
                    .revision
                    .checked_add(1)
                    .ok_or(E::Capacity)
            })?,
            callback: words[9] == u32::MAX,
            callback_owner: 0,
            tab_owner: 0,
            tab_key: 0,
            key: index.map_or(0, |index| self.tooltip(window).unwrap().tools[index].key),
            window: words[2],
            id: words[3],
            flags: words[1],
            rect: Rect32 {
                left: words[4] as i32,
                top: words[5] as i32,
                right: words[6] as i32,
                bottom: words[7] as i32,
            },
            data: if info.size >= 44 {
                words[10]
            } else {
                index.map_or(0, |index| self.tooltip(window).unwrap().tools[index].data)
            },
            text,
        };
        if tool.text.iter().any(|&unit| matches!(unit, 10 | 13)) {
            return Err(E::Win32(50));
        }
        self.tooltip_mut(window)?.prepare(&tool, index)?;
        Ok(Some((index, tool, adding)))
    }
    fn get_tool(
        &self,
        window: WindowHandle32,
        args: &[u32],
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<u32, E> {
        let (size, owner, id) = tool_key(ram, args[3])?;
        let tip = self.tooltip(window)?;
        let item = match args[1] {
            0x40e | 0x43a => tip.tools.get(args[2] as usize),
            0x40f | 0x43b => tip.current(),
            _ => tip.find(owner, id).map(|index| &tip.tools[index]),
        };
        let Some(tool) = item else {
            return Ok(0);
        };
        let output = memory::words::<1>(ram, args[3].checked_add(36).ok_or(E::Win32(87))?)?[0];
        let wide = matches!(args[1], 0x435 | 0x438 | 0x43a | 0x43b);
        if tool.callback && output != 0 {
            return Err(E::Win32(50));
        }
        let text = if output != 0 {
            memory::encoded_text(
                if matches!(args[1], 0x40b | 0x438) && args[2] != 0 {
                    args[2] as usize
                } else {
                    tool.text.len().saturating_mul(4) + 1
                },
                &tool.text,
                wide,
                call,
            )?
            .0
        } else {
            Vec::new()
        };
        if matches!(args[1], 0x40b | 0x438) {
            if !text.is_empty() {
                ram.write(GuestAddress(output), &text)?;
            }
            return Ok(0);
        }
        let mut bytes = [0; 48];
        for (out, value) in bytes.chunks_exact_mut(4).zip([
            size as u32,
            tool.flags,
            tool.window,
            tool.id,
            tool.rect.left as u32,
            tool.rect.top as u32,
            tool.rect.right as u32,
            tool.rect.bottom as u32,
            0,
            output,
            tool.data,
            0,
        ]) {
            out.copy_from_slice(&value.to_le_bytes());
        }
        ram.write_spans(&[
            (GuestAddress(args[3]), &bytes[..size]),
            (GuestAddress(output), &text),
        ])?;
        Ok(1)
    }
}
