use super::{
    User32CallContext32, User32Callback32, User32Context32, User32Outcome32,
    callback::Continuation32,
};
use crate::windows::{
    user32::User32Error32,
    window::{WindowHandle32, WindowMessage32},
};

#[derive(Clone, Copy)]
pub(super) enum ShowStage32 {
    Show,
    DeactivateNonClient,
    Deactivate,
    ActivateNonClient,
    Activate,
}

#[derive(Clone, Copy)]
pub(super) struct Show32 {
    window: WindowHandle32,
    thread: u32,
    result: u32,
    visible: bool,
    activate: bool,
    previous: Option<WindowHandle32>,
    target: Option<WindowHandle32>,
    stage: ShowStage32,
}

impl User32Context32 {
    pub(super) fn activate_created_window(
        &mut self,
        window: WindowHandle32,
        thread: u32,
    ) -> Result<User32Outcome32, User32Error32> {
        let data = self.state.record(window)?;
        let activate = data.style & 0x4000_0000 == 0
            && data.ex_style & 0x0800_0000 == 0
            && self.windows().get(window)?.description.enabled;
        let show = Show32 {
            window,
            thread,
            result: window.get(),
            visible: true,
            activate,
            previous: self.windows().active(),
            target: Some(window),
            stage: ShowStage32::Show,
        };
        self.pending.last_mut().unwrap().continuation = Continuation32::Show(show);
        self.advance_show_activation(show)
    }

    pub(super) fn show_window(
        &mut self,
        args: &[u32],
        context: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        self.check_continuation()?;
        let window = self.owned(args[0], context.thread_id)?;
        let mut record = self.windows().get(window)?;
        let mut data = self.state.record(window)?;
        if data.destroying || self.pending_destroy_contains(window) {
            return Err(User32Error32::Win32(1400));
        }
        let visible = match args[1] {
            0 => false,
            1 | 4 | 5 | 8 | 9 | 10 => true,
            _ => return Err(User32Error32::Win32(50)),
        };
        let activate = visible
            && matches!(args[1], 1 | 5 | 9 | 10)
            && data.style & 0x4000_0000 == 0
            && data.ex_style & 0x0800_0000 == 0
            && record.description.enabled;
        let previous = self.windows().active();
        let deactivate = !visible && previous == Some(window);
        let target = if activate {
            Some(window)
        } else if deactivate {
            None
        } else {
            previous
        };
        let old = record.description.visible;
        let show = Show32 {
            window,
            thread: context.thread_id,
            result: u32::from(old),
            visible,
            activate: activate || deactivate,
            previous,
            target,
            stage: ShowStage32::Show,
        };
        if old == visible {
            if !show.activate || previous == target {
                return Ok(User32Outcome32::Return(show.result));
            }
            self.start(Continuation32::Show(show))?;
            return self.advance_show_activation(show);
        }
        record.description.visible = visible;
        if visible {
            data.style |= 0x1000_0000;
        } else {
            data.style &= !0x1000_0000;
        }
        self.state
            .windows_mut()
            .update_description(window, record.description)?;
        self.state.update(data)?;
        self.start(Continuation32::Show(show))
    }

    pub(super) fn show_outcome(&mut self, show: Show32) -> Result<User32Outcome32, User32Error32> {
        let (window, message, wparam, lparam) = match show.stage {
            ShowStage32::Show => (show.window, 0x18, u32::from(show.visible), 0),
            ShowStage32::DeactivateNonClient | ShowStage32::Deactivate => (
                show.previous.ok_or(User32Error32::InvalidRequest)?,
                if matches!(show.stage, ShowStage32::DeactivateNonClient) {
                    0x86
                } else {
                    6
                },
                0,
                super::raw(show.target),
            ),
            ShowStage32::ActivateNonClient | ShowStage32::Activate => (
                show.target.ok_or(User32Error32::InvalidRequest)?,
                if matches!(show.stage, ShowStage32::ActivateNonClient) {
                    0x86
                } else {
                    6
                },
                1,
                super::raw(show.previous),
            ),
        };
        Ok(User32Outcome32::Callback(User32Callback32 {
            count: 4,
            procedure: self.windows().get(window)?.description.procedure,
            arguments: [window.get(), message, wparam, lparam],
            continuation: self.pending.last().unwrap().token,
        }))
    }

    pub(super) fn complete_show(
        &mut self,
        mut show: Show32,
    ) -> Result<User32Outcome32, User32Error32> {
        match show.stage {
            ShowStage32::Show => self.advance_show_activation(show),
            ShowStage32::DeactivateNonClient => {
                if self.windows().active() != show.previous {
                    return self.finish_show(show, false);
                }
                show.stage = ShowStage32::Deactivate;
                self.set_show(show)
            }
            ShowStage32::Deactivate => {
                if self.windows().active() != show.previous || !self.show_target_valid(show) {
                    return self.finish_show(show, false);
                }
                if show.target.is_none()
                    && let Some(target) = self.show_focus_target(show)
                {
                    self.windows().check_set_focus(show.thread, target)?;
                }
                self.state
                    .windows_mut()
                    .update_active(show.thread, show.target)?;
                if show.target.is_none() {
                    self.finish_show(show, true)
                } else {
                    show.stage = ShowStage32::ActivateNonClient;
                    self.set_show(show)
                }
            }
            ShowStage32::ActivateNonClient => {
                if self.windows().active() != show.target || !self.show_target_valid(show) {
                    return self.finish_show(show, false);
                }
                show.stage = ShowStage32::Activate;
                self.set_show(show)
            }
            ShowStage32::Activate => {
                let focused =
                    self.windows().active() == show.target && self.show_target_valid(show);
                self.finish_show(show, focused)
            }
        }
    }

    fn advance_show_activation(
        &mut self,
        mut show: Show32,
    ) -> Result<User32Outcome32, User32Error32> {
        if !show.activate || !self.show_target_valid(show) {
            return self.finish_show(show, false);
        }
        show.previous = self.windows().active();
        if show.previous == show.target {
            return self.finish_show(show, false);
        }
        if show.previous.is_some() {
            show.stage = ShowStage32::DeactivateNonClient;
        } else {
            self.state
                .windows_mut()
                .update_active(show.thread, show.target)?;
            show.stage = ShowStage32::ActivateNonClient;
        }
        self.set_show(show)
    }

    fn show_target_valid(&self, show: Show32) -> bool {
        show.target.is_none_or(|target| {
            self.windows()
                .get(target)
                .is_ok_and(|window| window.description.enabled && window.description.visible)
                && self.state.record(target).is_ok_and(|data| !data.destroying)
                && !self.pending_destroy_contains(target)
        })
    }

    fn set_show(&mut self, show: Show32) -> Result<User32Outcome32, User32Error32> {
        self.pending.last_mut().unwrap().continuation = Continuation32::Show(show);
        self.next_outcome()
    }

    pub(super) fn show_completion_focus_target(
        &self,
        show: Show32,
    ) -> Option<(u32, Option<WindowHandle32>)> {
        let completes_activation = match show.stage {
            ShowStage32::Activate => self.windows().active() == show.target,
            ShowStage32::Deactivate if show.target.is_none() => {
                self.windows().active() == show.previous
            }
            _ => false,
        };
        (completes_activation && self.show_target_valid(show))
            .then(|| self.show_focus_target(show))
            .flatten()
            .map(|target| (show.thread, target))
    }

    fn show_focus_target(&self, show: Show32) -> Option<Option<WindowHandle32>> {
        let focus = self.windows().focus();
        if let Some(target) = show.target {
            if focus.is_some_and(|focus| self.state.top_level(focus).ok() == Some(target)) {
                return None;
            }
            Some(Some(target))
        } else if focus.is_some_and(|focus| self.state.top_level(focus).ok() == Some(show.window)) {
            Some(None)
        } else {
            None
        }
    }

    fn finish_show(
        &mut self,
        show: Show32,
        update_focus: bool,
    ) -> Result<User32Outcome32, User32Error32> {
        if update_focus && let Some(target) = self.show_focus_target(show) {
            self.state.windows_mut().set_focus(show.thread, target)?;
            let mut events: [Option<WindowMessage32>; 6] = [None; 6];
            for event in &mut events {
                *event = self.state.windows_mut().pop_message();
            }
            if events[0].is_some() {
                self.pending.last_mut().unwrap().continuation = Continuation32::Sequence {
                    events,
                    index: 0,
                    result: show.result,
                };
                return self.next_outcome();
            }
        }
        self.pending.pop();
        Ok(User32Outcome32::Return(show.result))
    }
}
