mod input;
use super::*;
use crate::process::windows::user32::{callback::Continuation32, memory::Scratch32};
use crate::windows::user32::User32Api32;
#[derive(Clone, Copy)]
enum Action32 {
    Set {
        order: usize,
    },
    Begin {
        divider: bool,
        origin: i32,
        width: i32,
    },
    Move {
        full: bool,
    },
    End,
    Click {
        double: bool,
        divider: bool,
    },
    Cancel,
}
#[derive(Clone, Copy)]
enum Stage32 {
    Notify,
    Changing,
    Apply,
    Changed,
    Capture,
    Release,
    Finish,
}
#[derive(Clone, Copy)]
enum Child32 {
    Notify,
    Changing,
    Changed,
    Capture,
    Release,
}
#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct HeaderCall32 {
    window: WindowHandle32,
    parent: Option<WindowHandle32>,
    key: u64,
    call: User32CallContext32,
    scratch: Scratch32,
    action: Action32,
    stage: Stage32,
    wide: bool,
    result: u32,
    child: Option<(u64, Child32)>,
    completed: Option<u64>,
}
impl HeaderCall32 {
    pub(in crate::process::windows::user32) fn scratch(self) -> Scratch32 {
        self.scratch
    }
}
impl User32Context32 {
    pub(super) fn begin_header_change(
        &mut self,
        window: WindowHandle32,
        index: usize,
        item: HeaderItem32,
        order: usize,
        mask: u32,
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        self.start_header_call(
            window,
            index,
            &item,
            mask,
            Action32::Set { order },
            Stage32::Notify,
            ram,
            call,
        )
    }
    fn start_header_call(
        &mut self,
        window: WindowHandle32,
        index: usize,
        item: &HeaderItem32,
        mask: u32,
        action: Action32,
        stage: Stage32,
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        self.prepare_thread_slots()?;
        if self.pending.len() + 2 > self.continuation_limit {
            return Err(E::Capacity);
        }
        self.next_token.checked_add(2).ok_or(E::Capacity)?;
        let header = self.header(window)?;
        let wide = header.unicode;
        let order = header.position(index).ok_or(E::Win32(87))?;
        let key = header.items[index].key;
        let record = self.state.record(window)?;
        let parent = record.parent;
        let id = record.menu;
        let (text, _) = memory::encoded_text(
            item.text
                .len()
                .saturating_mul(call.ansi.maximum_char_size() as usize)
                .saturating_add(1),
            &item.text,
            wide,
            call,
        )?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(60 + text.len())
            .map_err(|_| E::Allocation)?;
        bytes.resize(60, 0);
        bytes.extend_from_slice(&text);
        let values = [
            window.get(),
            id,
            0,
            index as u32,
            0,
            0,
            mask,
            item.width as u32,
            0,
            0,
            0,
            item.format,
            item.data,
            0,
            order as u32,
        ];
        for (out, value) in bytes[..60].chunks_exact_mut(4).zip(values) {
            out.copy_from_slice(&value.to_le_bytes());
        }
        let scratch = Scratch32::allocate(ram, self.scratch_bounds, &bytes)?;
        self.start(Continuation32::Header(HeaderCall32 {
            window,
            parent,
            key,
            call,
            scratch,
            action,
            stage,
            wide,
            result: 0,
            child: None,
            completed: None,
        }))
    }
    fn header_call(&self, token: u64) -> Result<HeaderCall32, E> {
        let pending = self
            .pending
            .last()
            .filter(|pending| pending.token == token)
            .ok_or(E::InvalidToken)?;
        let Continuation32::Header(call) = pending.continuation else {
            return Err(E::InvalidToken);
        };
        Ok(call)
    }
    fn publish_header_call(&mut self, token: u64, call: HeaderCall32) {
        self.pending
            .iter_mut()
            .find(|pending| pending.token == token)
            .expect("same header continuation owner")
            .continuation = Continuation32::Header(call);
    }
    pub(in crate::process::windows::user32) fn header_wait(&self, token: u64) -> bool {
        self.pending.last().is_some_and(|pending| {
            pending.token == token && matches!(pending.continuation, Continuation32::Header(_))
        })
    }
    pub(in crate::process::windows::user32) fn pending_header_message(
        &self,
        token: u64,
    ) -> Result<Option<[u32; 4]>, E> {
        let call = self.header_call(token)?;
        if call.child.is_some() {
            return Err(E::InvalidToken);
        }
        if !matches!(
            call.stage,
            Stage32::Notify | Stage32::Changing | Stage32::Changed
        ) || self.windows().get(call.window).is_err()
        {
            return Ok(None);
        }
        let Some(parent) = call
            .parent
            .filter(|&parent| self.windows().get(parent).is_ok())
        else {
            return Ok(None);
        };
        Ok(Some([
            parent.get(),
            0x4e,
            self.state.record(call.window)?.menu,
            call.scratch.address.0,
        ]))
    }
    pub(in crate::process::windows::user32) fn preview_header_input_flags(
        &self,
        token: u64,
    ) -> Result<Option<(bool, bool)>, E> {
        if !self.header_wait(token) {
            return Ok(None);
        }
        let call = self.header_call(token)?;
        let captured = match call.stage {
            Stage32::Capture => Some(true),
            Stage32::Release if self.windows().capture() == Some(call.window) => Some(false),
            _ => None,
        };
        if let Some(captured) = captured {
            self.windows().check_set_capture(
                call.call.thread_id,
                if captured { Some(call.window) } else { None },
            )?;
        }
        Ok(captured.map(|captured| (self.windows().focus().is_some(), captured)))
    }
    fn header_notification_code(call: HeaderCall32) -> i32 {
        let offset = match call.stage {
            Stage32::Changing => 0,
            Stage32::Changed => 1,
            _ => match call.action {
                Action32::Set { .. } => 0,
                Action32::Begin { .. } => 6,
                Action32::Move { .. } => 8,
                Action32::End => 7,
                Action32::Click {
                    double: true,
                    divider: true,
                } => 5,
                Action32::Click { double: true, .. } => 3,
                Action32::Click { .. } => 2,
                Action32::Cancel => 0,
            },
        };
        -300 - if call.wide { 20 } else { 0 } - offset
    }
    fn after_header_notification(call: &mut HeaderCall32, kind: Child32, result: u32) {
        call.stage = match kind {
            Child32::Capture => Stage32::Apply,
            Child32::Release => Stage32::Finish,
            Child32::Changed => {
                if matches!(call.action, Action32::End) {
                    Stage32::Release
                } else {
                    Stage32::Finish
                }
            }
            Child32::Changing if result != 0 => {
                if matches!(call.action, Action32::End) {
                    Stage32::Release
                } else {
                    Stage32::Finish
                }
            }
            Child32::Changing => Stage32::Apply,
            Child32::Notify => match call.action {
                Action32::Set { .. } if result != 0 => Stage32::Finish,
                Action32::Begin { .. } if result != 0 => Stage32::Finish,
                Action32::Move { .. } if result != 0 => Stage32::Release,
                Action32::Set { .. } => Stage32::Apply,
                Action32::Begin { .. } => Stage32::Capture,
                Action32::Move { full: true } | Action32::End => Stage32::Changing,
                Action32::Move { full: false } => Stage32::Apply,
                Action32::Click { double: true, .. } => Stage32::Finish,
                Action32::Click { .. } | Action32::Cancel => Stage32::Release,
            },
        };
    }
    pub(in crate::process::windows::user32) fn resume_header_call(
        &mut self,
        token: u64,
        ram: &mut AddressSpace,
    ) -> Result<User32Outcome32, E> {
        let mut call = self.header_call(token)?;
        call.scratch.check(ram)?;
        if call.child.is_some() {
            return Err(E::InvalidToken);
        }
        if self.windows().get(call.window).is_err() {
            call.stage = Stage32::Finish;
        }
        match call.stage {
            Stage32::Notify | Stage32::Changing | Stage32::Changed => {
                let target = self
                    .header(call.window)?
                    .items
                    .iter()
                    .position(|item| item.key == call.key);
                if let Some(index) = target {
                    let code = Self::header_notification_code(call);
                    let values = [
                        (8, code as u32),
                        (12, index as u32),
                        (
                            20,
                            if matches!(call.action, Action32::Click { .. }) {
                                0
                            } else {
                                call.scratch.address.0 + 24
                            },
                        ),
                        (32, call.scratch.address.0 + 60),
                    ];
                    let bytes = values.map(|(offset, value)| {
                        (
                            GuestAddress(call.scratch.address.0 + offset),
                            value.to_le_bytes(),
                        )
                    });
                    ram.write_spans(
                        &bytes
                            .iter()
                            .map(|(at, value)| (*at, value.as_slice()))
                            .collect::<Vec<_>>(),
                    )?;
                    if let Some(message) = self.pending_header_message(token)? {
                        let kind = match call.stage {
                            Stage32::Changing => Child32::Changing,
                            Stage32::Changed => Child32::Changed,
                            _ => Child32::Notify,
                        };
                        let outcome = self.send_message(&message, call.call)?;
                        let child = child_token(outcome).ok_or(E::InvalidRequest)?;
                        call.child = Some((child, kind));
                        call.completed = None;
                        self.publish_header_call(token, call);
                        return Ok(outcome);
                    }
                    let kind = match call.stage {
                        Stage32::Changing => Child32::Changing,
                        Stage32::Changed => Child32::Changed,
                        _ => Child32::Notify,
                    };
                    Self::after_header_notification(&mut call, kind, 0);
                } else {
                    call.stage = Stage32::Release;
                }
            }
            Stage32::Capture | Stage32::Release => {
                let capture = matches!(call.stage, Stage32::Capture);
                if !capture {
                    if self.header(call.window)?.track.is_some() {
                        self.state.check_paint_change()?;
                        self.header_mut(call.window)?.track = None;
                        self.header_changed(call.window);
                    }
                    if self.windows().capture() != Some(call.window) {
                        call.stage = Stage32::Finish;
                        self.publish_header_call(token, call);
                        return Ok(User32Outcome32::Wait {
                            continuation: token,
                        });
                    }
                }
                let arguments = [call.window.get()];
                let outcome = self.prepare_arguments(
                    if capture {
                        User32Api32::SetCapture
                    } else {
                        User32Api32::ReleaseCapture
                    },
                    if capture { &arguments } else { &[] },
                    ram,
                    call.call,
                )?;
                let kind = if capture {
                    Child32::Capture
                } else {
                    Child32::Release
                };
                if let Some(child) = child_token(outcome) {
                    call.child = Some((child, kind));
                    call.completed = None;
                    self.publish_header_call(token, call);
                    return Ok(outcome);
                }
                Self::after_header_notification(&mut call, kind, 0);
            }
            Stage32::Apply => {
                if let Some(index) = self
                    .header(call.window)?
                    .items
                    .iter()
                    .position(|item| item.key == call.key)
                {
                    self.state.check_paint_change()?;
                    match call.action {
                        Action32::Set { order } => {
                            let (item, _, mask) = self.read_header_item(
                                call.window,
                                Some(index),
                                call.scratch.address.0 + 24,
                                call.wide,
                                ram,
                                call.call,
                            )?;
                            self.header_mut(call.window)?.items[index] = item;
                            if mask & 0x80 != 0 {
                                self.header_mut(call.window)?.move_item(index, order);
                            }
                            call.result = 1;
                            call.stage = Stage32::Changed;
                        }
                        Action32::Begin {
                            divider,
                            origin,
                            width,
                        } => {
                            if self.windows().capture() == Some(call.window) {
                                self.header_mut(call.window)?.track = Some(HeaderTrack32 {
                                    key: call.key,
                                    divider,
                                    origin,
                                    width,
                                    proposed: width,
                                });
                            }
                            call.stage = Stage32::Finish;
                        }
                        Action32::Move { .. } | Action32::End => {
                            let full = matches!(call.action, Action32::Move { full: true });
                            let width =
                                memory::words::<1>(ram, call.scratch.address.0 + 28)?[0] as i32;
                            let old = &self.header(call.window)?.items[index];
                            let proposal = HeaderItem32 {
                                key: old.key,
                                width,
                                text: Vec::new(),
                                format: old.format,
                                data: old.data,
                            };
                            self.header(call.window)?
                                .check_item(Some(index), &proposal)?;
                            if let Some(track) = self.header_mut(call.window)?.track.as_mut() {
                                track.proposed = width;
                            }
                            if matches!(call.action, Action32::End) || full {
                                self.header_mut(call.window)?.items[index].width = width;
                                call.stage = Stage32::Changed;
                            } else {
                                call.stage = Stage32::Finish;
                            }
                        }
                        _ => call.stage = Stage32::Release,
                    }
                    self.header_changed(call.window);
                } else {
                    call.stage = Stage32::Release;
                }
            }
            Stage32::Finish => {
                call.scratch.release(ram)?;
                self.pending.pop();
                return Ok(User32Outcome32::Return(call.result));
            }
        }
        self.publish_header_call(token, call);
        Ok(User32Outcome32::Wait {
            continuation: token,
        })
    }
    pub(in crate::process::windows::user32) fn complete_header_child(
        &mut self,
        child: u64,
        outcome: User32Outcome32,
    ) -> Result<User32Outcome32, E> {
        let User32Outcome32::Return(result) = outcome else {
            return Ok(outcome);
        };
        let Some(pending) = self.pending.last() else {
            return Ok(outcome);
        };
        let Continuation32::Header(mut call) = pending.continuation else {
            return Ok(outcome);
        };
        let Some((expected, kind)) = call.child else {
            return Ok(outcome);
        };
        if expected != child {
            return Ok(outcome);
        }
        let token = pending.token;
        call.child = None;
        call.completed = Some(child);
        Self::after_header_notification(&mut call, kind, result);
        self.publish_header_call(token, call);
        Ok(User32Outcome32::Wait {
            continuation: token,
        })
    }
    pub(in crate::process::windows::user32) fn retry_header_completion(
        &self,
        child: u64,
    ) -> Option<User32Outcome32> {
        let pending = self.pending.last()?;
        let Continuation32::Header(call) = pending.continuation else {
            return None;
        };
        (call.child.is_none() && call.completed == Some(child)).then_some(User32Outcome32::Wait {
            continuation: pending.token,
        })
    }
}
fn child_token(outcome: User32Outcome32) -> Option<u64> {
    match outcome {
        User32Outcome32::Callback(callback) => Some(callback.continuation),
        User32Outcome32::Wait { continuation } => Some(continuation),
        _ => None,
    }
}
