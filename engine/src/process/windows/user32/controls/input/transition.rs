use super::super::super::callback::Continuation32;
use super::*;
use crate::memory::AddressSpace;
#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct Focus32 {
    pub window: WindowHandle32,
    pub arguments: [u32; 4],
    pub call: User32CallContext32,
    pub metrics: InputMetrics32,
    pub events: [Option<WindowMessage32>; 6],
    pub index: usize,
    pub ready: bool,
}
#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct Close32 {
    pub window: WindowHandle32,
    pub popup: Option<WindowHandle32>,
    pub events: [Option<WindowMessage32>; 6],
    pub after: [Option<WindowMessage32>; 6],
    pub index: usize,
    pub ready: bool,
    pub result: u32,
}
impl User32Context32 {
    pub(super) fn check_control_continuation(&self) -> Result<(), E> {
        if self.pending.last().is_some_and(|pending| {
            matches!(
                pending.continuation,
                Continuation32::ControlFocus(Focus32 { ready: true, .. })
            )
        }) {
            Ok(())
        } else {
            self.check_continuation().map(|_| ())
        }
    }
    pub(super) fn control_start(&mut self, next: Continuation32) -> Result<User32Outcome32, E> {
        if self.pending.last().is_some_and(|pending| {
            matches!(
                pending.continuation,
                Continuation32::ControlFocus(Focus32 { ready: true, .. })
            )
        }) {
            self.pending.last_mut().unwrap().continuation = next;
            self.next_outcome()
        } else {
            self.start(next)
        }
    }
    pub(in crate::process::windows::user32) fn control_sequence(
        &mut self,
        events: [Option<WindowMessage32>; 6],
        result: u32,
    ) -> Result<User32Outcome32, E> {
        if events[0].is_none() {
            return Ok(User32Outcome32::Return(result));
        }
        self.control_start(Continuation32::Sequence {
            events,
            index: 0,
            result,
        })
    }
    pub(super) fn begin_control_focus(
        &mut self,
        window: WindowHandle32,
        args: &[u32],
        call: User32CallContext32,
        metrics: InputMetrics32,
    ) -> Result<User32Outcome32, E> {
        let arguments = args.try_into().map_err(|_| E::InvalidRequest)?;
        self.check_control_continuation()?;
        self.windows()
            .check_focus_capture(call.thread_id, Some(Some(window)), None)?;
        let events =
            self.commit_control_transition(window, call.thread_id, Some(Some(window)), None, &[]);
        self.control_start(Continuation32::ControlFocus(Focus32 {
            window,
            arguments,
            call,
            metrics,
            events,
            index: 0,
            ready: false,
        }))
    }
    pub(in crate::process::windows::user32) fn complete_control_focus(
        &mut self,
        mut focus: Focus32,
        _ram: &mut AddressSpace,
    ) -> Result<User32Outcome32, E> {
        if !focus.ready
            && focus
                .events
                .get(focus.index + 1)
                .copied()
                .flatten()
                .is_some()
        {
            focus.index += 1;
            self.pending.last_mut().unwrap().continuation = Continuation32::ControlFocus(focus);
            return self.next_outcome();
        }
        focus.ready = true;
        self.pending.last_mut().unwrap().continuation = Continuation32::ControlFocus(focus);
        let kind = self
            .state
            .record(focus.window)
            .ok()
            .filter(|data| !data.destroying)
            .and_then(|data| self.state.standard_kind(data.class_atom));
        let Some(kind) = kind else {
            self.pending.pop();
            return Ok(User32Outcome32::Return(0));
        };
        let outcome = self
            .control_input_message(
                focus.window,
                kind,
                &focus.arguments,
                focus.call,
                focus.metrics,
            )?
            .unwrap_or(User32Outcome32::Return(0));
        if matches!(outcome, User32Outcome32::Return(_))
            && self.pending.last().is_some_and(|pending| {
                matches!(pending.continuation, Continuation32::ControlFocus(_))
            })
        {
            self.pending.pop();
        }
        Ok(outcome)
    }
    pub(in crate::process::windows::user32) fn complete_control_close(
        &mut self,
        mut close: Close32,
    ) -> Result<User32Outcome32, E> {
        if !close.ready
            && close
                .events
                .get(close.index + 1)
                .copied()
                .flatten()
                .is_some()
        {
            close.index += 1;
            self.pending.last_mut().unwrap().continuation = Continuation32::ControlClose(close);
            return self.next_outcome();
        }
        close.ready = true;
        self.pending.last_mut().unwrap().continuation = Continuation32::ControlClose(close);
        if self.windows().get(close.window).is_err() {
            self.pending.pop();
            return Ok(User32Outcome32::Return(close.result));
        }
        let popup = close
            .popup
            .filter(|popup| self.windows().get(*popup).is_ok());
        self.state
            .check_control_changes(1 + u64::from(popup.is_some()))?;
        if let Some(popup) = popup {
            let mut description = self.windows().get(popup)?.description;
            description.visible = false;
            self.state
                .windows_mut()
                .update_description(popup, description)
                .expect("popup checked");
            self.state
                .invalidate_control(popup)
                .expect("close invalidation preflighted");
        }
        let state = self.state.control(close.window).expect("combo checked");
        state.dropped = false;
        state.opening = false;
        state.closing = false;
        self.state
            .invalidate_control(close.window)
            .expect("close invalidation preflighted");
        self.pending.last_mut().unwrap().continuation = Continuation32::Sequence {
            events: close.after,
            index: 0,
            result: close.result,
        };
        self.next_outcome()
    }
}
