use super::super::User32Context32;
use super::context::{User32Input32, mouse_key, normalize_modifiers};
use crate::{
    input::{
        InputKind, keyboard,
        source::{InputCursor32, SourceEvent32, SourceKind32},
    },
    process::{EngineInstance, HostError, MediaError},
    windows::{
        user32::{Message32, Point32, QueuedKey32, User32Error32},
        window::WindowHandle32,
    },
};

pub(super) struct InputDelivery32 {
    input: User32Input32,
    messages: [(Message32, Option<QueuedKey32>); 4],
    count: usize,
    layout: Option<(u32, crate::input::keyboard::KeyboardLayout32)>,
    raw: Option<crate::windows::rawinput::PreparedRawPacket32>,
}

impl EngineInstance {
    pub fn pending_user32_input(&self) -> bool {
        let (Some(context), Some(media)) = (self.user32.as_ref(), self.media.as_ref()) else {
            return false;
        };
        context.input.cursor != Some(media.input.source().tail())
    }

    pub(in crate::process) fn sync_user32_input(&mut self) -> Result<(), HostError> {
        let (Some(context), Some(media)) = (self.user32.as_mut(), self.media.as_ref()) else {
            return Ok(());
        };
        let source = media.input.source();
        if context.input.cursor.is_none() {
            context.input.cursor = Some(InputCursor32 {
                epoch: source.tail().epoch,
                sequence: 0,
            });
            context.input.focus = context.windows().focus().map_or(0, WindowHandle32::get);
            context.input.capture = context.windows().capture().map_or(0, WindowHandle32::get);
        }
        let (origin, _) = self.cpu_clock.monotonic_bounds()?;
        loop {
            let mut records = [SourceEvent32::default(); 1];
            let read = source
                .read(context.input.cursor.unwrap(), &mut records)
                .map_err(|error| HostError::Media(MediaError::Input(error)))?;
            if read.overflow {
                return Err(HostError::User32(User32Error32::Win32(1816)));
            }
            if read.count == 0 {
                break;
            }
            let delivery = context
                .prepare_input(records[0], origin)
                .map_err(HostError::User32)?;
            let mut messages = [Message32::default(); 4];
            for (index, (message, _)) in delivery.messages[..delivery.count].iter().enumerate() {
                messages[index] = *message;
            }
            let mut rows = [(0, 0, 0, 0); 4];
            let mut row_count = 0;
            for message in &messages[..delivery.count] {
                if rows[..row_count]
                    .iter()
                    .any(|row| row.0 == message.thread_id)
                {
                    continue;
                }
                let (current, fresh, generation) = context
                    .state
                    .project_input_messages(message.thread_id, &messages[..delivery.count])
                    .map_err(HostError::User32)?;
                rows[row_count] = (message.thread_id, current, fresh, generation);
                row_count += 1;
            }
            let prepared = self
                .synchronization
                .as_ref()
                .filter(|_| row_count != 0)
                .map(|runtime| {
                    runtime
                        .context()
                        .prepare_messages_for_threads(&rows[..row_count])
                })
                .transpose()
                .map_err(HostError::Synchronization)?;
            context.commit_input(delivery);
            context.input.cursor = Some(read.next);
            if let Some(prepared) = prepared {
                self.synchronization
                    .as_mut()
                    .unwrap()
                    .context_mut()
                    .commit_signals(prepared);
            }
        }
        Ok(())
    }
}

impl User32Context32 {
    pub(super) fn prepare_input(
        &self,
        event: SourceEvent32,
        origin: u64,
    ) -> Result<InputDelivery32, User32Error32> {
        let mut input = self.input;
        let mut messages = [(Message32::default(), None); 4];
        let mut count = 0;
        let mut layout = None;
        let time_ms = (event.timestamp_ns.saturating_sub(origin) / 1_000_000) as u32;
        let mut push = |window, message, wparam, lparam, point, key| {
            messages[count] = (
                Message32 {
                    window,
                    message,
                    wparam,
                    lparam,
                    point,
                    time_ms,
                    thread_id: WindowHandle32::try_from(window)
                        .ok()
                        .and_then(|handle| self.windows().get(handle).ok())
                        .map_or(1, |window| window.description.thread_id),
                },
                key,
            );
            count += 1;
        };
        match event.kind {
            SourceKind32::Physical(InputKind::Focus { focused }) => {
                input.physical_focus = focused;
                if !focused {
                    for value in &mut input.asynchronous {
                        *value &= 1;
                    }
                    input.recent = [false; 256];
                    input.physical_capture = false;
                }
            }
            SourceKind32::Physical(InputKind::Capture { captured }) => {
                input.physical_capture = captured
            }
            SourceKind32::Window { focus, capture } => {
                input.focus = focus;
                input.capture = capture;
                if let Ok(window) =
                    WindowHandle32::try_from(focus).and_then(|handle| self.windows().get(handle))
                {
                    input.delivery_layout =
                        self.delivered_thread_layout(window.description.thread_id);
                }
            }
            SourceKind32::Policy {
                mouse_exclusive,
                no_windows_key: _,
            } => {
                if input.mouse_exclusive != mouse_exclusive {
                    input.cursor_revision = input
                        .cursor_revision
                        .checked_add(1)
                        .ok_or(User32Error32::Capacity)?;
                    input.mouse_exclusive = mouse_exclusive;
                    input.cursor_request = Some(super::User32CursorRequest32 {
                        revision: input.cursor_revision,
                        changes: 4 | input.cursor_request.map_or(0, |request| request.changes),
                        position: input.query_point,
                        clip: input.clip,
                        visible: input.show_count >= 0 && !mouse_exclusive,
                        cursor: input.cursor_handle,
                    });
                }
            }
            SourceKind32::CursorPolicy { x, y, bounds } => {
                input.delivery_clip =
                    bounds.map(
                        |(left, top, right, bottom)| crate::windows::user32::Rect32 {
                            left,
                            top,
                            right,
                            bottom,
                        },
                    );
                let previous = input.point;
                input.point = Point32 { x, y };
                let target = self.mouse_target(input.capture, input.focus, input.point);
                if previous != input.point
                    && target != 0
                    && input.physical_focus
                    && !input.mouse_exclusive
                {
                    push(
                        target,
                        0x200,
                        mouse_flags(&input.asynchronous, input.delivery_swapped),
                        pack_point(self.client_point(target, input.point)?),
                        input.point,
                        None,
                    );
                }
            }
            SourceKind32::ProcessTranslationPolicy {
                layout: value,
                swapped,
            } => {
                layout = Some((0, value));
                input.delivery_layout = value;
                input.delivery_swapped = swapped;
            }
            SourceKind32::ThreadTranslationPolicy {
                thread,
                layout: value,
                swapped,
            } => {
                layout = Some((thread, value));
                input.delivery_swapped = swapped;
                if WindowHandle32::try_from(input.focus)
                    .ok()
                    .and_then(|window| self.windows().get(window).ok())
                    .is_some_and(|window| window.description.thread_id == thread)
                {
                    input.delivery_layout = value;
                }
            }
            SourceKind32::TranslationPolicy {
                layout: value,
                swapped,
            } => {
                input.delivery_layout = value;
                input.delivery_swapped = swapped;
                if let Some(thread) = WindowHandle32::try_from(input.focus)
                    .ok()
                    .and_then(|handle| self.windows().get(handle).ok())
                    .map(|window| window.description.thread_id)
                {
                    layout = Some((thread, value));
                }
            }
            SourceKind32::Physical(InputKind::Key { code, pressed }) => {
                let key = keyboard::virtual_key_for_layout(input.delivery_layout, code);
                if key != 0 {
                    let repeated = input.asynchronous[key as usize] & 0x80 != 0;
                    if let Some(previous) = input.held_virtual_keys.get_mut(code as usize) {
                        if pressed {
                            *previous = key;
                        } else {
                            if *previous != key {
                                input.asynchronous[*previous as usize] &= 1;
                            }
                            *previous = 0;
                        }
                    }
                    let alt = input.asynchronous[0x12] & 0x80 != 0;
                    if pressed && !repeated && matches!(key, 0x14 | 0x90 | 0x91) {
                        input.asynchronous[key as usize] ^= 1;
                    }
                    input.asynchronous[key as usize] =
                        (input.asynchronous[key as usize] & 1) | (u8::from(pressed) << 7);
                    if pressed && !repeated {
                        input.recent[key as usize] = true;
                    }
                    normalize_modifiers(&mut input.asynchronous);
                    for (generic, left, right) in
                        [(0x10, 0xa0, 0xa1), (0x11, 0xa2, 0xa3), (0x12, 0xa4, 0xa5)]
                    {
                        if key as usize == left || key as usize == right {
                            input.recent[generic] |= pressed && !repeated;
                        }
                    }
                    if input.focus != 0 && self.live_input_window(input.focus) {
                        let system = alt || matches!(key, 0xa4 | 0xa5) || key == 0x79;
                        let message = match (system, pressed) {
                            (false, true) => 0x100,
                            (false, false) => 0x101,
                            (true, true) => 0x104,
                            (true, false) => 0x105,
                        };
                        let generic = match key {
                            0xa0 | 0xa1 => 0x10,
                            0xa2 | 0xa3 => 0x11,
                            0xa4 | 0xa5 => 0x12,
                            _ => key,
                        };
                        let lparam = 1
                            | (u32::from(code & 0x7f) << 16)
                            | (u32::from(code == 0x45 || code & 0x80 != 0 && code != 0xc5) << 24)
                            | (u32::from(alt) << 29)
                            | (u32::from(repeated || !pressed) << 30)
                            | (u32::from(!pressed) << 31);
                        push(
                            input.focus,
                            message,
                            u32::from(generic),
                            lparam,
                            input.point,
                            Some(QueuedKey32 {
                                virtual_key: key,
                                pressed,
                                repeated,
                            }),
                        );
                    }
                }
            }
            SourceKind32::Physical(InputKind::Button { button, pressed }) => {
                if let Some(physical_key) = mouse_key(button) {
                    let repeated = input.asynchronous[physical_key as usize] & 0x80 != 0;
                    input.asynchronous[physical_key as usize] = u8::from(pressed) << 7;
                    if pressed && !repeated {
                        input.recent[physical_key as usize] = true;
                    }
                    let button = if input.delivery_swapped && button < 2 {
                        button ^ 1
                    } else {
                        button
                    };
                    let target = self.mouse_target(input.capture, input.focus, input.point);
                    if target != 0 && !input.mouse_exclusive {
                        let mut message = match (button, pressed) {
                            (0, true) => 0x201,
                            (0, false) => 0x202,
                            (1, true) => 0x204,
                            (1, false) => 0x205,
                            (2, true) => 0x207,
                            (2, false) => 0x208,
                            (_, true) => 0x20b,
                            (_, false) => 0x20c,
                        };
                        if pressed && self.class_style(target)? & 8 != 0 {
                            if input
                                .last_click
                                .is_some_and(|(window, previous, point, time)| {
                                    window == target
                                        && previous == button
                                        && time_ms.wrapping_sub(time) <= input.double_click_ms
                                        && (i64::from(point.x) - i64::from(input.point.x)).abs()
                                            <= 2
                                        && (i64::from(point.y) - i64::from(input.point.y)).abs()
                                            <= 2
                                })
                            {
                                message += 2;
                                input.last_click = None;
                            } else {
                                input.last_click = Some((target, button, input.point, time_ms));
                            }
                        }
                        let wparam = mouse_flags(&input.asynchronous, input.delivery_swapped)
                            | if button >= 3 {
                                u32::from(button - 2) << 16
                            } else {
                                0
                            };
                        let point = self.client_point(target, input.point)?;
                        push(
                            target,
                            message,
                            wparam,
                            pack_point(point),
                            input.point,
                            Some(QueuedKey32 {
                                virtual_key: mouse_key(button).unwrap(),
                                pressed,
                                repeated,
                            }),
                        );
                    }
                }
            }
            SourceKind32::Cursor { x, y } => {
                input.point = delivery_point(Point32 { x, y }, input.delivery_clip);
                let target = self.mouse_target(input.capture, input.focus, input.point);
                if input.physical_focus && target != 0 && !input.mouse_exclusive {
                    push(
                        target,
                        0x200,
                        mouse_flags(&input.asynchronous, input.delivery_swapped),
                        pack_point(self.client_point(target, input.point)?),
                        input.point,
                        None,
                    );
                }
            }
            SourceKind32::Physical(InputKind::Relative { dx, dy, wheel }) => {
                input.point = delivery_point(
                    Point32 {
                        x: input.point.x.saturating_add(dx),
                        y: input.point.y.saturating_add(dy),
                    },
                    input.delivery_clip,
                );
                let target = self.mouse_target(input.capture, input.focus, input.point);
                if target != 0 && input.physical_focus && !input.mouse_exclusive {
                    let flags = mouse_flags(&input.asynchronous, input.delivery_swapped);
                    if dx != 0 || dy != 0 {
                        push(
                            target,
                            0x200,
                            flags,
                            pack_point(self.client_point(target, input.point)?),
                            input.point,
                            None,
                        );
                    }
                    if wheel != 0 {
                        push(
                            target,
                            0x20a,
                            flags | ((wheel as u32 & 0xffff) << 16),
                            pack_point(input.point),
                            input.point,
                            None,
                        );
                    }
                }
            }
            SourceKind32::Controller { .. } | SourceKind32::Connection { .. } => {}
        }
        if matches!(event.kind, SourceKind32::Physical(InputKind::Key { .. }))
            && self.raw_input.no_legacy(true)
            || matches!(
                event.kind,
                SourceKind32::Physical(InputKind::Button { .. } | InputKind::Relative { .. })
                    | SourceKind32::Cursor { .. }
                    | SourceKind32::CursorPolicy { .. }
            ) && self.raw_input.no_legacy(false)
        {
            count = 0;
        }
        let raw = if let SourceKind32::Physical(kind) = event.kind {
            let key = if let InputKind::Key { code, .. } = kind {
                match keyboard::virtual_key_for_layout(input.delivery_layout, code) {
                    0 => 255,
                    0xa0 | 0xa1 => 0x10,
                    0xa2 | 0xa3 => 0x11,
                    0xa4 | 0xa5 => 0x12,
                    key => u16::from(key),
                }
            } else {
                0
            };
            self.raw_input.prepare_packet(
                kind,
                key,
                input.asynchronous[0x12] & 0x80 != 0 || matches!(key, 0x12 | 0x79),
                input.focus,
                input.physical_focus,
                |raw| {
                    WindowHandle32::try_from(raw)
                        .ok()
                        .and_then(|handle| self.state.record(handle).ok())
                        .filter(|data| !data.destroying)
                        .and_then(|data| self.windows().get(data.handle).ok())
                        .map(|window| window.description.thread_id)
                },
            )?
        } else {
            None
        };
        if let Some(raw) = &raw {
            let packet = raw.packet();
            messages.copy_within(0..count, 1);
            messages[0] = (
                Message32 {
                    window: packet.window,
                    message: 0xff,
                    wparam: u32::from(packet.bytes[12]),
                    lparam: packet.id,
                    time_ms,
                    point: input.point,
                    thread_id: packet.thread,
                },
                None,
            );
            count += 1;
        }
        self.state.can_post(count)?;
        Ok(InputDelivery32 {
            input,
            messages,
            count,
            layout,
            raw,
        })
    }

    pub(super) fn commit_input(&mut self, delivery: InputDelivery32) {
        if let Some(raw) = delivery.raw {
            self.raw_input.commit_packet(raw);
        }
        if let Some((thread, layout)) = delivery.layout {
            if thread == 0 {
                for value in &mut self.layouts {
                    value.delivery = layout;
                }
            } else {
                self.commit_thread_layout_delivery(thread, layout);
            }
        }
        let mut keyboard = delivery.input.asynchronous;
        if delivery.input.delivery_swapped {
            keyboard.swap(1, 2);
        }
        for &(message, key) in &delivery.messages[..delivery.count] {
            if message.message == 0xff {
                self.state
                    .post_raw_input(message)
                    .expect("raw input delivery was preflighted");
            } else {
                self.state
                    .post_input(message, key, keyboard)
                    .expect("input delivery was preflighted");
            }
        }
        self.runtime_input.cursor = delivery.input.query_point;
        self.input = delivery.input;
    }

    #[cfg(test)]
    pub(in crate::process::windows::user32) fn deliver_input(
        &mut self,
        event: SourceEvent32,
        origin: u64,
    ) -> Result<(), User32Error32> {
        let delivery = self.prepare_input(event, origin)?;
        self.commit_input(delivery);
        Ok(())
    }

    fn class_style(&self, window: u32) -> Result<u32, User32Error32> {
        let data = self.state.record(WindowHandle32::try_from(window)?)?;
        Ok(self
            .state
            .class(data.instance, &[], Some(data.class_atom))?
            .style)
    }

    fn live_input_window(&self, window: u32) -> bool {
        WindowHandle32::try_from(window)
            .ok()
            .and_then(|handle| self.state.record(handle).ok())
            .is_some_and(|window| !window.destroying)
    }

    fn mouse_target(&self, capture: u32, focus: u32, point: Point32) -> u32 {
        if capture != 0 && self.live_input_window(capture) {
            return capture;
        }
        if focus != 0
            && self.live_input_window(focus)
            && self
                .state
                .record(WindowHandle32::try_from(focus).unwrap())
                .is_ok_and(|window| {
                    point.x >= window.outer.left
                        && point.x < window.outer.right
                        && point.y >= window.outer.top
                        && point.y < window.outer.bottom
                })
        {
            return focus;
        }
        0
    }

    pub(super) fn client_point(
        &self,
        window: u32,
        point: Point32,
    ) -> Result<Point32, User32Error32> {
        let origin = self
            .state
            .record(WindowHandle32::try_from(window)?)?
            .client_origin;
        Ok(Point32 {
            x: point.x.saturating_sub(origin.x),
            y: point.y.saturating_sub(origin.y),
        })
    }
}

pub(super) fn pack_point(point: Point32) -> u32 {
    (point.x as u32 & 0xffff) | ((point.y as u32 & 0xffff) << 16)
}
fn mouse_flags(keys: &[u8; 256], swapped: bool) -> u32 {
    u32::from(keys[if swapped { 2 } else { 1 }] & 0x80 != 0)
        | (u32::from(keys[if swapped { 1 } else { 2 }] & 0x80 != 0) << 1)
        | (u32::from(keys[0x10] & 0x80 != 0) << 2)
        | (u32::from(keys[0x11] & 0x80 != 0) << 3)
        | (u32::from(keys[4] & 0x80 != 0) << 4)
        | (u32::from(keys[5] & 0x80 != 0) << 5)
        | (u32::from(keys[6] & 0x80 != 0) << 6)
}

fn delivery_point(point: Point32, clip: Option<crate::windows::user32::Rect32>) -> Point32 {
    clip.map_or(point, |clip| Point32 {
        x: point.x.clamp(clip.left, clip.right.saturating_sub(1)),
        y: point.y.clamp(clip.top, clip.bottom.saturating_sub(1)),
    })
}
