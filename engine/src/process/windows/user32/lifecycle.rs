use super::{
    User32CallContext32, User32Context32, User32Outcome32,
    callback::{Continuation32, CreateStage32, PositionStage32},
    classes::class_name,
    memory::{self, Scratch32},
};
use crate::{
    memory::AddressSpace,
    windows::{
        user32::{Point32, Rect32, User32Api32, User32Error32, WindowData32},
        window::{WindowDescription32, WindowHandle32, WindowMessage32},
    },
};

impl User32Context32 {
    pub(super) fn create_window(
        &mut self,
        api: User32Api32,
        args: &[u32],
        memory: &mut AddressSpace,
        context: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        self.check_continuation()?;
        let (name, atom) = class_name(memory, args[1], api.wide(), context.ansi)?;
        let instance = if args[10] == 0 {
            context.instance
        } else {
            args[10]
        };
        let class = self.state.class(instance, &name, atom)?;
        let (class_atom, procedure, unicode, extra_bytes) = (
            class.atom,
            class.procedure,
            class.unicode,
            class.extra_bytes,
        );
        let (procedure, unicode) = if self.state.standard_kind(class.atom).is_some() {
            let procedures = self.builtin_procedures.ok_or(User32Error32::Win32(1407))?;
            (procedures[usize::from(api.wide())], api.wide())
        } else {
            (procedure, unicode)
        };
        if unicode != api.wide() {
            return Err(User32Error32::Win32(50));
        }
        let mut parent = if args[8] == 0 {
            None
        } else {
            Some(self.owned(args[8], context.thread_id)?)
        };
        if args[3] & 0x4000_0000 == 0 {
            parent = parent
                .map(|parent| self.state.top_level(parent))
                .transpose()?;
        }
        if parent.is_some_and(|parent| {
            self.pending_destroy_contains(parent)
                || self.state.record(parent).is_ok_and(|data| data.destroying)
        }) {
            return Err(User32Error32::Win32(1400));
        }
        if args[3] & 0x4000_0000 != 0 && parent.is_none() {
            return Err(User32Error32::Win32(1406));
        }
        if args[9] != 0 && args[3] & 0x4000_0000 == 0 {
            return Err(User32Error32::Win32(50));
        }
        let caption = if args[2] != 0 {
            memory::string(memory, args[2], api.wide(), 32_767, context.ansi)?
        } else {
            Vec::new()
        };
        let special = args[3] & 0xc000_0000 != 0;
        let (x, y) = if args[4] == 0x8000_0000 {
            (0, 0)
        } else {
            (args[4] as i32, args[5] as i32)
        };
        let (width, height) = if args[6] == 0x8000_0000 {
            if special { (0, 0) } else { (640, 480) }
        } else {
            (args[6] as i32, args[7] as i32)
        };
        let origin = if args[3] & 0x4000_0000 != 0 {
            self.state
                .record(parent.ok_or(User32Error32::Win32(1406))?)?
                .client_origin
        } else {
            Point32::default()
        };
        let screen_x = x.checked_add(origin.x).ok_or(User32Error32::Win32(87))?;
        let screen_y = y.checked_add(origin.y).ok_or(User32Error32::Win32(87))?;
        let outer = checked_rect(screen_x, screen_y, width, height)?;
        let description = WindowDescription32 {
            thread_id: context.thread_id,
            procedure,
            extent: outer.extent()?,
            visible: false,
            enabled: args[3] & 0x0800_0000 == 0,
        };
        let handle = self.state.check_create(description)?;
        let mut extra = Vec::new();
        extra
            .try_reserve_exact(extra_bytes as usize)
            .map_err(|_| User32Error32::Allocation)?;
        extra.resize(extra_bytes as usize, 0);
        let extra = extra.into_boxed_slice();
        let mut bytes = [0; 64];
        for (index, value) in [
            args[11],
            instance,
            args[9],
            super::raw(parent),
            height as u32,
            width as u32,
            y as u32,
            x as u32,
            args[3],
            args[2],
            args[1],
            args[0],
            outer.left as u32,
            outer.top as u32,
            outer.right as u32,
            outer.bottom as u32,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        let scratch = Scratch32::allocate(memory, self.scratch_bounds, &bytes)?;
        let data = WindowData32 {
            handle,
            class_atom,
            instance,
            style: args[3],
            ex_style: args[0],
            user_data: 0,
            parent,
            menu: args[9],
            outer,
            client_origin: Point32 {
                x: screen_x,
                y: screen_y,
            },
            unicode,
            destroying: false,
        };
        let published = self.state.publish_window(description, data, extra);
        debug_assert_eq!(handle, published);
        self.state
            .set_text(handle, caption)
            .expect("new window owns its caption slot");
        self.start(Continuation32::Create {
            window: handle,
            thread: context.thread_id,
            scratch,
            stage: CreateStage32::NonClientCreate,
            visible: args[3] & 0x1000_0000 != 0,
        })
    }

    pub(super) fn destroy_window(
        &mut self,
        raw: u32,
        context: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        let window = self.owned(raw, context.thread_id)?;
        self.begin_destroy(window, context.thread_id, 1, None, true, false)
    }

    pub(super) fn set_focus_capture(
        &mut self,
        api: User32Api32,
        args: &[u32],
        context: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        self.check_continuation()?;
        let handle = if api == User32Api32::ReleaseCapture || args[0] == 0 {
            None
        } else {
            Some(self.owned(args[0], context.thread_id)?)
        };
        if handle.is_some_and(|handle| {
            self.pending_destroy_contains(handle)
                || self
                    .state
                    .record(handle)
                    .is_ok_and(|record| record.destroying)
        }) {
            return Err(User32Error32::Win32(1400));
        }
        let old = if api == User32Api32::SetFocus {
            self.state
                .windows_mut()
                .set_focus(context.thread_id, handle)?
        } else {
            self.state
                .windows_mut()
                .set_capture(context.thread_id, handle)?
        };
        let mut events = [None; 6];
        for event in &mut events {
            *event = self.state.windows_mut().pop_message();
        }
        self.sequence(
            events,
            if api == User32Api32::ReleaseCapture {
                1
            } else {
                super::raw(old)
            },
        )
    }

    pub(super) fn move_window(
        &mut self,
        api: User32Api32,
        args: &[u32],
        memory: &mut AddressSpace,
        context: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        self.check_continuation()?;
        let window = self.owned(args[0], context.thread_id)?;
        let data = self.state.record(window)?;
        if data.destroying {
            return Err(User32Error32::Win32(1400));
        }
        let (x, y, width, height, flags) = if api == User32Api32::MoveWindow {
            (args[1], args[2], args[3], args[4], 0x0014)
        } else {
            (args[2], args[3], args[4], args[5], args[6])
        };
        check_position_flags(flags)?;
        let outer = self.position_rect(window, x, y, width, height, flags)?;
        let mut bytes = [0; 44];
        for (index, value) in [
            window.get(),
            0,
            x,
            y,
            width,
            height,
            flags,
            outer.left as u32,
            outer.top as u32,
            outer.right as u32,
            outer.bottom as u32,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        let scratch = Scratch32::allocate(memory, self.scratch_bounds, &bytes)?;
        let stage = if flags & 0x0400 != 0 {
            PositionStage32::NonClientSize
        } else {
            PositionStage32::Changing
        };
        self.start(Continuation32::Position {
            window,
            scratch,
            stage,
        })
    }

    pub(super) fn position_rect(
        &self,
        window: WindowHandle32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        flags: u32,
    ) -> Result<Rect32, User32Error32> {
        let data = self.state.record(window)?;
        let origin = if data.style & 0x4000_0000 != 0 {
            self.state
                .record(data.parent.ok_or(User32Error32::Win32(1400))?)?
                .client_origin
        } else {
            Point32::default()
        };
        let x = if flags & 2 == 0 {
            (x as i32)
                .checked_add(origin.x)
                .ok_or(User32Error32::Win32(87))? as u32
        } else {
            x
        };
        let y = if flags & 2 == 0 {
            (y as i32)
                .checked_add(origin.y)
                .ok_or(User32Error32::Win32(87))? as u32
        } else {
            y
        };
        position_rect(data.outer, x, y, width, height, flags)
    }

    pub(super) fn publish_position(
        &mut self,
        window: WindowHandle32,
        outer: Rect32,
        client: Rect32,
        flags: u32,
    ) -> Result<(), User32Error32> {
        let extent = client.extent()?;
        let mut data = self.state.record(window)?;
        let mut record = self.windows().get(window)?;
        if data.destroying {
            return Err(User32Error32::Win32(1400));
        }
        data.outer = outer;
        data.client_origin = Point32 {
            x: client.left,
            y: client.top,
        };
        record.description.extent = extent;
        if flags & 0x0040 != 0 {
            record.description.visible = true;
            data.style |= 0x1000_0000;
        }
        if flags & 0x0080 != 0 {
            record.description.visible = false;
            data.style &= !0x1000_0000;
        }
        let child_delta = self.state.check_move_children(window, data.client_origin)?;
        self.state
            .windows_mut()
            .update_description(window, record.description)?;
        self.state.update(data)?;
        self.state.move_children(window, child_delta);
        Ok(())
    }

    pub(super) fn def_window_proc_encoded(
        &mut self,
        args: &[u32],
        wide: bool,
        memory: &mut AddressSpace,
        context: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        if let Some(outcome) = self.window_text_message(args, wide, memory, context)? {
            return Ok(outcome);
        }
        self.def_window_proc(args, memory, context)
    }

    pub(super) fn def_window_proc(
        &mut self,
        args: &[u32],
        memory: &mut AddressSpace,
        context: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        if args[1] == 0xff {
            self.default_raw_input(args[0], context.thread_id, args[3])?;
            return Ok(User32Outcome32::Return(0));
        }
        if args[1] == 0x0081 {
            return Ok(User32Outcome32::Return(1));
        }
        if args[1] == 0x0086 {
            return Ok(User32Outcome32::Return(1));
        }
        if args[1] == 6 && args[2] & 0xffff != 0 && args[2] >> 16 == 0 {
            let handle = self.owned(args[0], context.thread_id)?;
            if self.windows().active() == Some(handle) {
                if self.pending_destroy_contains(handle) || self.state.record(handle)?.destroying {
                    return Err(User32Error32::Win32(1400));
                }
                self.check_continuation()?;
                self.state
                    .windows_mut()
                    .set_focus(context.thread_id, Some(handle))?;
                let mut events = [None; 6];
                for event in &mut events {
                    *event = self.state.windows_mut().pop_message();
                }
                return self.sequence(events, 0);
            }
        }
        if args[1] == 0x0010 {
            let outcome = self.destroy_window(args[0], context)?;
            if let Some(plan) = self.destructions.last_mut() {
                plan.result = 0;
            }
            return Ok(match outcome {
                User32Outcome32::Return(_) => User32Outcome32::Return(0),
                outcome => outcome,
            });
        }
        if args[1] == 0x0047 {
            self.check_continuation()?;
            let window = WindowHandle32::try_from(args[0])?;
            let record = self.windows().get(window)?;
            let position = self.state.client_position(window)?;
            let [hwnd, _, _, _, _, _, flags] = memory::words::<7>(memory, args[3])?;
            if hwnd != args[0] {
                return Err(User32Error32::Win32(87));
            }
            let mut events = [None; 6];
            let mut index = 0;
            if flags & 0x0002 == 0 {
                events[index] = Some(WindowMessage32 {
                    window,
                    procedure: record.description.procedure,
                    message: 0x0003,
                    wparam: 0,
                    lparam: (position.x as u32 & 0xffff) | ((position.y as u32) << 16),
                });
                index += 1;
            }
            if flags & 0x0001 == 0 {
                events[index] = Some(WindowMessage32 {
                    window,
                    procedure: record.description.procedure,
                    message: 0x0005,
                    wparam: 0,
                    lparam: (record.description.extent.width & 0xffff)
                        | (record.description.extent.height << 16),
                });
            }
            return self.sequence(events, 0);
        }
        if args[1] == 0x0083 {
            let handle = WindowHandle32::try_from(args[0])?;
            let data = self.state.record(handle)?;
            let mut rect = memory::rect(memory, args[3])?;
            let border = if data.style & 0x0004_0000 != 0 {
                8
            } else if data.style & 0x0080_0000 != 0 {
                1
            } else {
                0
            };
            let caption = if data.style & 0x00c0_0000 == 0x00c0_0000 {
                23
            } else {
                0
            };
            rect.left = rect.left.saturating_add(border).min(rect.right);
            rect.right = rect.right.saturating_sub(border).max(rect.left);
            rect.top = rect.top.saturating_add(border + caption).min(rect.bottom);
            rect.bottom = rect.bottom.saturating_sub(border).max(rect.top);
            memory::write_rect(memory, args[3], rect)?;
        }
        Ok(User32Outcome32::Return(0))
    }
}

fn checked_rect(x: i32, y: i32, width: i32, height: i32) -> Result<Rect32, User32Error32> {
    if width < 0 || height < 0 {
        return Err(User32Error32::Win32(87));
    }
    Ok(Rect32 {
        left: x,
        top: y,
        right: x.checked_add(width).ok_or(User32Error32::Win32(87))?,
        bottom: y.checked_add(height).ok_or(User32Error32::Win32(87))?,
    })
}

pub(super) fn check_position_flags(flags: u32) -> Result<(), User32Error32> {
    if flags & !0x07ff != 0 || flags & 0x0004 == 0 || flags & 0x00c0 == 0x00c0 {
        return Err(User32Error32::Win32(50));
    }
    Ok(())
}

pub(super) fn position_rect(
    old: Rect32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    flags: u32,
) -> Result<Rect32, User32Error32> {
    checked_rect(
        if flags & 2 != 0 { old.left } else { x as i32 },
        if flags & 2 != 0 { old.top } else { y as i32 },
        if flags & 1 != 0 {
            old.right - old.left
        } else {
            width as i32
        },
        if flags & 1 != 0 {
            old.bottom - old.top
        } else {
            height as i32
        },
    )
}
