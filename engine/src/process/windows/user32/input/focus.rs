use super::super::{
    User32CallContext32, User32Context32,
    callback::{Continuation32, CreateStage32},
};
use crate::windows::{CallFrame32, user32::User32Api32};

impl User32Context32 {
    pub(crate) fn preview_input_flags(
        &self,
        api: User32Api32,
        frame: &CallFrame32,
        context: User32CallContext32,
    ) -> Option<(bool, bool)> {
        if frame.convention() != api.convention() || frame.stack_words() != api.stack_words() {
            return None;
        }
        let focused = self.windows().focus().is_some();
        let captured = self.windows().capture().is_some();
        let args = frame.arguments();
        match api {
            User32Api32::SetFocus => {
                let handle = if args[0] == 0 {
                    None
                } else {
                    Some(self.owned(args[0], context.thread_id).ok()?)
                };
                if handle.is_some_and(|handle| {
                    self.pending_destroy_contains(handle)
                        || self.state.record(handle).is_ok_and(|data| data.destroying)
                }) {
                    return None;
                }
                self.windows()
                    .check_set_focus(context.thread_id, handle)
                    .ok()?;
                Some((handle.is_some(), captured))
            }
            User32Api32::SetCapture | User32Api32::ReleaseCapture => {
                let handle = if api == User32Api32::ReleaseCapture || args[0] == 0 {
                    None
                } else {
                    Some(self.owned(args[0], context.thread_id).ok()?)
                };
                if handle.is_some_and(|handle| {
                    self.pending_destroy_contains(handle)
                        || self.state.record(handle).is_ok_and(|data| data.destroying)
                }) {
                    return None;
                }
                self.windows()
                    .check_set_capture(context.thread_id, handle)
                    .ok()?;
                Some((focused, handle.is_some()))
            }
            User32Api32::DestroyWindow => self.preview_destroy_flags(args[0], context.thread_id),
            User32Api32::DefWindowProcA | User32Api32::DefWindowProcW
                if args[1] == 6 && args[2] & 0xffff != 0 && args[2] >> 16 == 0 =>
            {
                let handle = self.owned(args[0], context.thread_id).ok()?;
                if self.windows().active() != Some(handle)
                    || self.pending_destroy_contains(handle)
                    || self.state.record(handle).ok()?.destroying
                {
                    return None;
                }
                self.windows()
                    .check_set_focus(context.thread_id, Some(handle))
                    .ok()?;
                Some((true, captured))
            }
            User32Api32::DefWindowProcA | User32Api32::DefWindowProcW if args[1] == 0x10 => {
                self.preview_destroy_flags(args[0], context.thread_id)
            }
            _ => None,
        }
    }

    pub(crate) fn preview_completion_input_flags(
        &self,
        token: u64,
        result: u32,
    ) -> Option<(bool, bool)> {
        let pending = self
            .pending
            .last()
            .filter(|pending| pending.token == token)?;
        if let Continuation32::ControlFocus(focus) = pending.continuation
            && (focus.ready
                || focus
                    .events
                    .get(focus.index + 1)
                    .copied()
                    .flatten()
                    .is_none())
            && let Ok(data) = self.state.record(focus.window)
            && let Some(kind) = self.state.standard_kind(data.class_atom)
        {
            return self.control_input_flags(focus.window, kind, &focus.arguments, true);
        }
        if let Continuation32::ControlDropdown(drop) = pending.continuation {
            let show = (drop.ready || drop.events.get(drop.index + 1).copied().flatten().is_none())
                && self
                    .state
                    .control_state(drop.window)
                    .is_ok_and(|state| state.opening && state.popup == Some(drop.popup))
                && self
                    .windows()
                    .get(drop.window)
                    .is_ok_and(|record| record.description.enabled)
                && self.windows().get(drop.popup).is_ok();
            if show {
                if let Some(capture) = drop.capture {
                    self.windows()
                        .check_set_capture(drop.thread, Some(capture))
                        .ok()?;
                }
                return Some((
                    self.windows().focus().is_some(),
                    drop.capture.is_some() || self.windows().capture().is_some(),
                ));
            }
        }
        if let Continuation32::Show(show) = pending.continuation {
            let (thread, target) = self.show_completion_focus_target(show)?;
            self.windows().check_set_focus(thread, target).ok()?;
            return Some((target.is_some(), self.windows().capture().is_some()));
        }
        if let Continuation32::AppActivation {
            resume: super::super::activation::AppResume32::Show(show),
            ..
        } = pending.continuation
        {
            let (thread, target) = self.show_completion_focus_target(show)?;
            self.windows().check_set_focus(thread, target).ok()?;
            return Some((target.is_some(), self.windows().capture().is_some()));
        }
        if let Continuation32::Destroy { plan } = pending.continuation {
            return self.destroy_completion_flags(plan);
        }
        let Continuation32::Create {
            window,
            thread,
            stage,
            ..
        } = pending.continuation
        else {
            return None;
        };
        let rejected = match stage {
            CreateStage32::Hook(_) => result != 0,
            CreateStage32::NonClientCreate => result == 0,
            CreateStage32::Create => result == u32::MAX,
            _ => false,
        };
        if !rejected {
            return None;
        }
        self.preview_destroy_flags(window.get(), thread)
    }

    fn preview_destroy_flags(&self, raw: u32, thread: u32) -> Option<(bool, bool)> {
        let handle = self.owned(raw, thread).ok()?;
        let focused = self.destroy_focus_target(handle);
        self.windows().check_set_focus(thread, focused).ok()?;
        Some((focused.is_some(), self.windows().capture().is_some()))
    }
}
