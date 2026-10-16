use super::super::{User32CallContext32, User32Context32, User32Outcome32, memory};
use super::{User32CursorRequest32, delivery::pack_point};
use crate::{
    memory::{AddressSpace, GuestAddress},
    process::{EngineInstance, HostError},
    windows::{
        user32::{Point32, Rect32, User32Api32, User32Error32},
        window::WindowHandle32,
    },
};

impl User32Context32 {
    pub(super) fn pointer_query(
        &mut self,
        api: User32Api32,
        args: &[u32],
        memory: &mut AddressSpace,
        context: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        use User32Api32::*;
        let value = match api {
            GetCursorPos => {
                write_point(memory, args[0], self.input.query_point)?;
                1
            }
            SetCursorPos => {
                let revision = self.cursor_request_revision()?;
                self.input.query_point = self.input.clip_point(Point32 {
                    x: args[0] as i32,
                    y: args[1] as i32,
                });
                if self.input.cursor.is_none() {
                    self.input.point = self.input.query_point;
                }
                self.runtime_input.cursor = self.input.query_point;
                self.publish_cursor_request(revision, 1);
                1
            }
            ClipCursor => {
                let clip = if args[0] == 0 {
                    None
                } else {
                    let rect = memory::rect(memory, args[0])?;
                    if rect.left >= rect.right || rect.top >= rect.bottom {
                        return Err(User32Error32::Win32(87));
                    }
                    Some(rect)
                };
                let revision = self.cursor_request_revision()?;
                self.input.clip = clip;
                self.input.query_point = self.input.clip_point(self.input.query_point);
                if self.input.cursor.is_none() {
                    self.input.delivery_clip = clip;
                }
                if self.input.cursor.is_none() {
                    self.input.point = self.input.query_point;
                }
                self.runtime_input.cursor = self.input.query_point;
                self.publish_cursor_request(revision, 3);
                1
            }
            GetClipCursor => {
                memory::write_rect(
                    memory,
                    args[0],
                    self.input.clip.unwrap_or(self.input.desktop),
                )?;
                1
            }
            ShowCursor => {
                let revision = self.cursor_request_revision()?;
                let count = if args[0] != 0 {
                    self.input.show_count.checked_add(1)
                } else {
                    self.input.show_count.checked_sub(1)
                }
                .ok_or(User32Error32::Capacity)?;
                self.input.show_count = count;
                self.publish_cursor_request(revision, 4);
                count as u32
            }
            SetCursor => {
                if args[0] != 0 {
                    return Err(User32Error32::Win32(50));
                }
                let revision = self.cursor_request_revision()?;
                let previous = self.input.cursor_handle;
                self.input.cursor_handle = args[0];
                self.publish_cursor_request(revision, 8);
                previous
            }
            GetCursor => self.input.cursor_handle,
            GetDoubleClickTime => self.input.double_click_ms,
            SetDoubleClickTime => {
                self.input.double_click_ms = if args[0] == 0 { 500 } else { args[0].min(5000) };
                1
            }
            SwapMouseButton => {
                let previous = self.input.swapped;
                self.input.swapped = args[0] != 0;
                if self.input.cursor.is_none() {
                    self.input.delivery_swapped = self.input.swapped;
                }
                u32::from(previous)
            }
            GetSystemMetrics => match args[0] {
                0 | 78 => {
                    (i64::from(self.input.desktop.right) - i64::from(self.input.desktop.left))
                        as u32
                }
                1 | 79 => {
                    (i64::from(self.input.desktop.bottom) - i64::from(self.input.desktop.top))
                        as u32
                }
                76 => self.input.desktop.left as u32,
                77 => self.input.desktop.top as u32,
                19 | 75 | 80 | 81 => 1,
                23 => u32::from(self.input.swapped),
                36 | 37 => 4,
                43 => 5,
                11..=14 => 32,
                49 | 50 => 16,
                63 | 67 | 0x1000 => 0,
                _ => return Err(User32Error32::Win32(50)),
            },
            GetQueueStatus => {
                if args[0] & !0x7fff != 0 {
                    return Err(User32Error32::Win32(87));
                }
                let (current, new, _) = self.state.queue_status(context.thread_id);
                self.state.observe_queue(context.thread_id, args[0]);
                ((current & args[0]) << 16) | (new & args[0])
            }
            GetInputState => {
                let (_, fresh, _) = self.state.queue_status(context.thread_id);
                self.state.observe_queue(context.thread_id, 5);
                u32::from(fresh & 5 != 0)
            }
            ScreenToClient | ClientToScreen => {
                let [x, y] = memory::words(memory, args[1])?;
                let origin = self
                    .state
                    .record(WindowHandle32::try_from(args[0])?)?
                    .client_origin;
                let sign = if api == ScreenToClient { -1 } else { 1 };
                let point = Point32 {
                    x: i32::try_from(i64::from(x as i32) + i64::from(origin.x) * sign)
                        .map_err(|_| User32Error32::Win32(87))?,
                    y: i32::try_from(i64::from(y as i32) + i64::from(origin.y) * sign)
                        .map_err(|_| User32Error32::Win32(87))?,
                };
                write_point(memory, args[1], point)?;
                1
            }
            GetMessagePos => pack_point(self.input.message.point),
            GetMessageTime => self.input.message.time_ms,
            GetMessageExtraInfo => self.input.extra_info,
            SetMessageExtraInfo => {
                let previous = self.input.extra_info;
                self.input.extra_info = args[0];
                previous
            }
            GetActiveWindow => self.windows().active().map_or(0, WindowHandle32::get),
            GetForegroundWindow => {
                if self.input.query_focus {
                    self.windows().active().map_or(0, WindowHandle32::get)
                } else {
                    0
                }
            }
            _ => return Err(User32Error32::InvalidRequest),
        };
        Ok(User32Outcome32::Return(value))
    }

    fn cursor_request_revision(&self) -> Result<u64, User32Error32> {
        self.input
            .cursor_revision
            .checked_add(1)
            .ok_or(User32Error32::Capacity)
    }
    fn publish_cursor_request(&mut self, revision: u64, changes: u32) {
        self.input.cursor_revision = revision;
        self.input.cursor_request = Some(User32CursorRequest32 {
            revision,
            changes: changes
                | self
                    .input
                    .cursor_request
                    .map_or(0, |request| request.changes),
            position: self.input.query_point,
            clip: self.input.clip,
            visible: self.input.show_count >= 0 && !self.input.mouse_exclusive,
            cursor: self.input.cursor_handle,
        });
    }

    pub(in crate::process) fn observe_wait_queue(&mut self, thread: u32, mask: u32) {
        self.state.observe_queue(thread, mask);
    }
}

impl EngineInstance {
    pub fn pending_user32_cursor(&self) -> Option<User32CursorRequest32> {
        self.user32
            .as_ref()
            .and_then(|context| context.input.cursor_request)
    }
    pub fn acknowledge_user32_cursor(&mut self, revision: u64) -> Result<(), HostError> {
        self.memory()?;
        let context = self.user32.as_mut().ok_or(HostError::InvalidRequest)?;
        if !context
            .input
            .cursor_request
            .is_some_and(|request| request.revision == revision)
        {
            return Err(HostError::User32(User32Error32::InvalidToken));
        }
        context.input.cursor_request = None;
        Ok(())
    }
    pub fn set_user32_desktop(&mut self, bounds: Rect32) -> Result<(), HostError> {
        self.memory()?;
        bounds.extent().map_err(HostError::User32)?;
        if bounds.left >= bounds.right || bounds.top >= bounds.bottom {
            return Err(HostError::InvalidRequest);
        }
        let source = self.prepare_user32_window_event()?;
        let context = self.user32.as_mut().ok_or(HostError::InvalidRequest)?;
        let old = context.input.query_point;
        context.input.desktop = bounds;
        context.input.query_point = context.input.clip_point(old);
        context.runtime_input.cursor = context.input.query_point;
        if context.input.cursor.is_none() {
            context.input.point = context.input.query_point;
        }
        self.commit_user32_cursor_event(source, true);
        Ok(())
    }
}

fn write_point(
    memory: &mut AddressSpace,
    address: u32,
    point: Point32,
) -> Result<(), User32Error32> {
    let mut bytes = [0; 8];
    bytes[..4].copy_from_slice(&point.x.to_le_bytes());
    bytes[4..].copy_from_slice(&point.y.to_le_bytes());
    memory.write(GuestAddress(address), &bytes)?;
    Ok(())
}
