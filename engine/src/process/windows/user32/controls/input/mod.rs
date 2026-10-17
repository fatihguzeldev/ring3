mod button;
mod combo;
mod transition;
pub(in crate::process::windows::user32) use combo::Dropdown32;
pub(in crate::process::windows::user32) use transition::{Close32, Focus32};
mod lists;
use super::super::{User32CallContext32, User32Context32, User32Outcome32};
use crate::{
    process::{EngineInstance, HostError},
    windows::{
        controls::ControlKind32,
        user32::User32Error32 as E,
        window::{WindowHandle32, WindowMessage32},
    },
};
#[derive(Clone, Copy, Default)]
pub(in crate::process::windows::user32) struct InputMetrics32 {
    pub height: i32,
    pub caret: Option<usize>,
}

pub(super) fn input_message(message: u32) -> bool {
    matches!(
        message,
        1 | 0x30 | 0x47 | 7 | 8 | 0x0a | 0x1f | 0x87 | 0x100 | 0x101 | 0x104 | 0x105 | 0x200
            ..=0x203 | 0x215 | 0x14f | 0x157 | 0xf5
    )
}
impl EngineInstance {
    pub(in crate::process::windows::user32) fn execute_builtin_control_message(
        &mut self,
        args: &[u32],
        wide: bool,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, HostError> {
        if !input_message(args[1]) {
            return self
                .user32
                .as_mut()
                .ok_or(HostError::InvalidRequest)?
                .control_message(
                    args,
                    wide,
                    self.memory.as_mut().ok_or(HostError::Closed)?,
                    call,
                )
                .map_err(HostError::User32);
        }
        let context = self.user32.as_ref().ok_or(HostError::InvalidRequest)?;
        let window = context
            .owned(args[0], call.thread_id)
            .map_err(HostError::User32)?;
        let data = context.state.record(window).map_err(HostError::User32)?;
        let kind = context
            .state
            .standard_kind(data.class_atom)
            .ok_or(HostError::InvalidRequest)?;
        let font = if args[1] == 0x30 {
            args[2]
        } else {
            context
                .state
                .control_state(window)
                .map_err(HostError::User32)?
                .font
        };
        let metrics = if let Some(gdi) = self.gdi.as_ref().filter(|_| {
            matches!(
                kind,
                ControlKind32::ListBox | ControlKind32::ComboBox | ControlKind32::Edit
            )
        }) {
            let hit = if kind == ControlKind32::Edit && args[1] == 0x201 {
                Some((
                    context.state.text(window).map_err(HostError::User32)?,
                    i32::from(args[3] as i16) - 2,
                ))
            } else {
                None
            };
            let (height, caret) = gdi
                .control_input_metrics(font, hit)
                .map_err(HostError::Gdi)?;
            InputMetrics32 { height, caret }
        } else {
            InputMetrics32::default()
        };
        let flags = context.control_input_flags(window, kind, args, false);
        let input = flags
            .map(|(focus, capture)| self.prepare_windows_input_flags(focus, capture))
            .transpose()?
            .flatten();
        let identities = self.user32_window_identities();
        let source = if flags.is_some() {
            self.prepare_user32_window_event()?
        } else {
            None
        };
        let queue = self.prepare_user32_queue_change()?;
        let context = self.user32.as_mut().unwrap();
        let outcome = match context
            .control_input_message(window, kind, args, call, metrics)
            .map_err(HostError::User32)?
        {
            Some(outcome) => outcome,
            None => context
                .control_message(
                    args,
                    wide,
                    self.memory.as_mut().ok_or(HostError::Closed)?,
                    call,
                )
                .map_err(HostError::User32)?,
        };
        let actual = (
            context.windows().focus().is_some(),
            context.windows().capture().is_some(),
        );
        self.commit_windows_input_flags(if flags == Some(actual) { input } else { None });
        self.commit_user32_window_event(source, identities);
        self.commit_user32_queue_change(queue);
        let _ = self.sync_windows_input();
        Ok(outcome)
    }
}
impl User32Context32 {
    pub(in crate::process::windows::user32) fn control_input_flags(
        &self,
        window: WindowHandle32,
        kind: ControlKind32,
        args: &[u32],
        after_focus: bool,
    ) -> Option<(bool, bool)> {
        let mut flags = (
            self.windows().focus().is_some(),
            self.windows().capture().is_some(),
        );
        let enabled = self.windows().get(window).ok()?.description.enabled;
        let captured = self.windows().capture() == Some(window);
        if !after_focus
            && matches!(args[1], 0x201 | 0x203)
            && enabled
            && self.windows().focus() != Some(window)
            && !(kind == ControlKind32::Button && self.state.record(window).ok()?.style & 15 == 7)
        {
            return Some((true, flags.1));
        }
        match (kind, args[1]) {
            (ControlKind32::Button, 0x201 | 0x203) => flags.1 = true,
            (ControlKind32::Button, 0x100) if args[2] == 32 => flags.1 = true,
            (ControlKind32::ListBox, 0x201 | 0x203) if enabled => flags.1 = true,
            (ControlKind32::Edit, 0x201) if enabled => {}
            (ControlKind32::ComboBox, message) => {
                let data = self.state.record(window).ok()?;
                let state = self.state.control_state(window).ok()?;
                let dropped = state.dropped;
                let captured = captured
                    || state
                        .popup
                        .is_some_and(|popup| self.windows().capture() == Some(popup));
                match message {
                    0x14f if data.style & 3 != 1 => {
                        flags.1 = if enabled && args[2] != 0 {
                            flags.1
                        } else if captured {
                            false
                        } else {
                            flags.1
                        }
                    }
                    0x201 | 0x203 if enabled => {
                        let (x, y) = point(args[3]);
                        let top = state.item_height.checked_add(4)?;
                        let in_list = x >= 1
                            && x < (self.windows().get(window).ok()?.description.extent.width
                                as i32
                                - 1)
                            && y > top
                            && y < top.saturating_add(state.drop_height as i32);
                        flags.1 = if !dropped {
                            flags.1
                        } else if in_list {
                            true
                        } else if captured {
                            false
                        } else {
                            flags.1
                        };
                    }
                    0x202 if dropped && enabled => {
                        let (x, y) = point(args[3]);
                        let top = state.item_height.checked_add(4)?;
                        let index = if state.item_height > 0 && x >= 1 && y > top {
                            Some(
                                state.scroll.2.max(0) as usize
                                    + ((y - top - 1) / state.item_height) as usize,
                            )
                        } else {
                            None
                        };
                        if captured && index.is_some_and(|index| index < state.items.len()) {
                            flags.1 = false;
                        }
                    }
                    0x100 | 0x104 if enabled => {
                        if args[2] == 0x73 && message == 0x100
                            || message == 0x104 && matches!(args[2], 0x26 | 0x28)
                        {
                            flags.1 = !dropped || !captured;
                        } else if dropped && matches!(args[2], 13 | 27) && captured {
                            flags.1 = false;
                        } else {
                            return None;
                        }
                    }
                    8 | 0x1f | 0x0a if dropped && captured => flags.1 = false,
                    _ => return None,
                }
            }
            (_, 8 | 0x202 | 0x1f | 0x101 | 0x105 | 0x0a) if captured => flags.1 = false,
            _ => return None,
        }
        Some(flags)
    }
    pub(super) fn control_input_message(
        &mut self,
        window: WindowHandle32,
        kind: ControlKind32,
        args: &[u32],
        call: User32CallContext32,
        metrics: InputMetrics32,
    ) -> Result<Option<User32Outcome32>, E> {
        if !input_message(args[1]) {
            return Ok(None);
        }
        if self.state.record(window)?.destroying && !matches!(args[1], 8 | 0x1f | 0x215 | 0x0a) {
            return Err(E::Win32(1400));
        }
        if args[1] == 0x87 {
            return Ok(Some(User32Outcome32::Return(match kind {
                ControlKind32::Button => match self.state.record(window)?.style & 15 {
                    0 => 0x2020,
                    1 => 0x2010,
                    2 | 3 | 5 | 6 => 0x2080,
                    4 | 9 => 0x2040,
                    7 => 0x100,
                    _ => 0x2000,
                },
                ControlKind32::Edit => 0x89,
                ControlKind32::ListBox | ControlKind32::ComboBox => 0x81,
                _ => 0,
            })));
        }
        if kind == ControlKind32::ListBox && self.state.control_list_owner(window)?.is_some() {
            return self.list_input(window, args, call, metrics);
        }
        if !self.pending.last().is_some_and(|pending| {
            matches!(
                pending.continuation,
                super::super::callback::Continuation32::ControlFocus(Focus32 { ready: true, .. })
            )
        }) && matches!(args[1], 0x201 | 0x203)
            && matches!(
                kind,
                ControlKind32::Button
                    | ControlKind32::ListBox
                    | ControlKind32::ComboBox
                    | ControlKind32::Edit
            )
            && self.windows().get(window)?.description.enabled
            && self.windows().focus() != Some(window)
            && !(kind == ControlKind32::Button && self.state.record(window)?.style & 15 == 7)
        {
            return self
                .begin_control_focus(window, args, call, metrics)
                .map(Some);
        }
        match kind {
            ControlKind32::Button => self.button_input(window, args, call),
            ControlKind32::ListBox => self.list_input(window, args, call, metrics),
            ControlKind32::ComboBox => self.combo_input(window, args, call, metrics),
            ControlKind32::Edit
                if args[1] == 0x201 && self.windows().get(window)?.description.enabled =>
            {
                let caret = metrics.caret.ok_or(E::Win32(50))?;
                self.check_control_transition(window, call.thread_id, None, None, &[], 1)?;
                let events =
                    self.commit_control_transition(window, call.thread_id, None, None, &[]);
                self.state.control(window)?.selection = (caret, caret);
                self.state
                    .invalidate_control(window)
                    .expect("control invalidation preflighted");
                self.control_sequence(events, 0).map(Some)
            }
            _ => Ok(None),
        }
    }
    fn check_control_transition(
        &self,
        window: WindowHandle32,
        thread: u32,
        focus: Option<Option<WindowHandle32>>,
        capture: Option<Option<WindowHandle32>>,
        codes: &[u32],
        changes: u64,
    ) -> Result<(), E> {
        self.check_control_continuation()?;
        self.windows().check_focus_capture(thread, focus, capture)?;
        self.state.check_control_changes(changes)?;
        if let Some(parent) = self.state.record(window)?.parent {
            self.windows().get(parent)?;
        }
        if codes.len() > 3 {
            return Err(E::InvalidRequest);
        }
        Ok(())
    }
    fn commit_control_transition(
        &mut self,
        window: WindowHandle32,
        thread: u32,
        focus: Option<Option<WindowHandle32>>,
        capture: Option<Option<WindowHandle32>>,
        codes: &[u32],
    ) -> [Option<WindowMessage32>; 6] {
        if let Some(focus) = focus {
            self.state
                .windows_mut()
                .set_focus(thread, focus)
                .expect("control focus preflighted");
        }
        if let Some(capture) = capture {
            self.state
                .windows_mut()
                .set_capture(thread, capture)
                .expect("control capture preflighted");
        }
        let mut events = [None; 6];
        let mut count = 0;
        while let Some(event) = self.state.windows_mut().pop_message() {
            assert!(count < 6);
            events[count] = Some(event);
            count += 1;
        }
        let data = self
            .state
            .record(window)
            .expect("control remains live before callbacks");
        if let Some(parent) = data.parent {
            let procedure = self
                .windows()
                .get(parent)
                .expect("control parent preflighted")
                .description
                .procedure;
            for code in codes {
                assert!(count < 6);
                events[count] = Some(WindowMessage32 {
                    window: parent,
                    procedure,
                    message: 0x111,
                    wparam: (data.menu & 0xffff) | (*code << 16),
                    lparam: window.get(),
                });
                count += 1;
            }
        }
        events
    }
}
fn point(value: u32) -> (i32, i32) {
    (i32::from(value as i16), i32::from((value >> 16) as i16))
}

#[cfg(test)]
mod tests;
