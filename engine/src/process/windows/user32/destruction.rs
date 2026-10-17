use super::{
    User32Callback32, User32Context32, User32Outcome32,
    callback::{Continuation32, Pending32},
    memory::Scratch32,
};
use crate::{
    memory::AddressSpace,
    windows::{
        user32::User32Error32,
        window::{WindowHandle32, WindowMessage32},
    },
};

#[derive(Clone, Copy)]
enum Step32 {
    Message(WindowHandle32, u32, u32, u32),
    Capture(WindowHandle32),
    Remove(WindowHandle32),
}
#[derive(Clone, Copy)]
enum Work32 {
    Root(WindowHandle32, bool),
    Destroy(WindowHandle32, bool),
    DestroyBody(WindowHandle32, bool),
    Free(WindowHandle32),
    Finish(WindowHandle32),
}

pub(super) struct DestroyPlan32 {
    pub token: u64,
    pub root: WindowHandle32,
    thread: u32,
    steps: Vec<Step32>,
    cursor: usize,
    event: Option<WindowMessage32>,
    pub(super) result: u32,
    scratch: Option<Scratch32>,
}

impl DestroyPlan32 {
    pub(in crate::process::windows::user32) fn check_scratch(
        &self,
        memory: &AddressSpace,
    ) -> Result<(), User32Error32> {
        if let Some(scratch) = self.scratch {
            scratch.check(memory)?;
        }
        Ok(())
    }
    pub(in crate::process::windows::user32) fn scratch_range(
        &self,
    ) -> Option<crate::memory::PageRange> {
        self.scratch.map(Scratch32::range)
    }
}

impl User32Context32 {
    pub(super) fn pending_destroy_contains(&self, window: WindowHandle32) -> bool {
        self.destructions
            .iter()
            .any(|plan| self.state.destroy_tree_contains(window, plan.root))
    }

    pub(super) fn destroy_focus_target(&self, root: WindowHandle32) -> Option<WindowHandle32> {
        let focus = self.windows().focus()?;
        if !self.state.destroy_tree_contains(focus, root) {
            return Some(focus);
        }
        self.state.record(root).ok()?.parent.filter(|parent| {
            self.windows()
                .get(*parent)
                .is_ok_and(|window| window.description.enabled)
                && !self.pending_destroy_contains(*parent)
        })
    }

    pub(super) fn begin_destroy(
        &mut self,
        root: WindowHandle32,
        thread: u32,
        result: u32,
        scratch: Option<Scratch32>,
        send_destroy: bool,
        replace: bool,
    ) -> Result<User32Outcome32, User32Error32> {
        let token = if replace {
            self.pending
                .last()
                .ok_or(User32Error32::InvalidToken)?
                .token
        } else {
            self.check_continuation()?
        };
        if self.destructions.len() == self.continuation_limit {
            return Err(User32Error32::Capacity);
        }
        self.owned(root.get(), thread)?;
        let next_focus = self.destroy_focus_target(root);
        if self.windows().focus() != next_focus {
            self.windows().check_set_focus(thread, next_focus)?;
        }
        let count = self.windows().live_windows();
        let mut steps = Vec::new();
        steps
            .try_reserve_exact(count * 4 + 2)
            .map_err(|_| User32Error32::Allocation)?;
        let mut work = Vec::new();
        work.try_reserve_exact(count * 3 + 1)
            .map_err(|_| User32Error32::Allocation)?;
        work.push(Work32::Root(root, send_destroy));
        while let Some(item) = work.pop() {
            match item {
                Work32::Root(window, send) => {
                    self.owned(window.get(), thread)?;
                    work.push(Work32::Free(window));
                    work.push(Work32::Destroy(window, send));
                }
                Work32::Destroy(window, send) => {
                    work.push(Work32::DestroyBody(window, send));
                    for popup in self.state.related_windows(window, false) {
                        work.push(Work32::Root(popup, true));
                    }
                }
                Work32::DestroyBody(window, send) => {
                    self.owned(window.get(), thread)?;
                    if send {
                        steps.push(Step32::Message(window, 0x0002, 0, 0));
                    }
                    for child in self.state.related_windows(window, true).rev() {
                        work.push(Work32::Destroy(child, true));
                    }
                }
                Work32::Free(window) => {
                    work.push(Work32::Finish(window));
                    for child in self.state.related_windows(window, true).rev() {
                        work.push(Work32::Free(child));
                    }
                }
                Work32::Finish(window) => {
                    steps.push(Step32::Message(window, 0x0082, 0, 0));
                    steps.push(Step32::Capture(window));
                    steps.push(Step32::Remove(window));
                }
            }
        }
        let mut prefix = [None; 2];
        if self.windows().focus() != next_focus {
            self.state.windows_mut().set_focus(thread, next_focus)?;
            for event in &mut prefix {
                *event = self.state.windows_mut().pop_message();
            }
        }
        for event in prefix.into_iter().flatten().rev() {
            steps.insert(
                0,
                Step32::Message(event.window, event.message, event.wparam, event.lparam),
            );
        }
        self.destructions.push(DestroyPlan32 {
            token,
            root,
            thread,
            steps,
            cursor: 0,
            event: None,
            result,
            scratch,
        });
        if replace {
            self.pending.last_mut().unwrap().continuation = Continuation32::Destroy { plan: token };
        } else {
            self.next_token = token;
            self.pending.push(Pending32 {
                token,
                continuation: Continuation32::Destroy { plan: token },
                activation: None,
            });
        }
        self.advance_destroy(token, None)
    }

    pub(super) fn destroy_outcome(&self, token: u64) -> Result<User32Outcome32, User32Error32> {
        let plan = self
            .destructions
            .last()
            .filter(|plan| plan.token == token)
            .ok_or(User32Error32::InvalidToken)?;
        let event = plan.event.ok_or(User32Error32::InvalidToken)?;
        Ok(User32Outcome32::Callback(User32Callback32 {
            count: 4,
            procedure: event.procedure,
            arguments: [
                event.window.get(),
                event.message,
                event.wparam,
                event.lparam,
            ],
            continuation: token,
        }))
    }

    pub(super) fn complete_destroy(
        &mut self,
        token: u64,
        memory: &mut AddressSpace,
    ) -> Result<User32Outcome32, User32Error32> {
        let plan = self
            .destructions
            .last_mut()
            .filter(|plan| plan.token == token)
            .ok_or(User32Error32::InvalidToken)?;
        plan.event = None;
        self.advance_destroy(token, Some(memory))
    }

    fn advance_destroy(
        &mut self,
        token: u64,
        mut memory: Option<&mut AddressSpace>,
    ) -> Result<User32Outcome32, User32Error32> {
        loop {
            let plan = self
                .destructions
                .last()
                .filter(|plan| plan.token == token)
                .ok_or(User32Error32::InvalidToken)?;
            let step = plan.steps.get(plan.cursor).copied();
            let thread = plan.thread;
            let Some(step) = step else {
                if let Some(scratch) = plan.scratch {
                    scratch.release(memory.as_deref_mut().ok_or(User32Error32::InvalidRequest)?)?;
                }
                let result = plan.result;
                self.destructions.pop();
                self.pending.pop();
                return Ok(User32Outcome32::Return(result));
            };
            let window = match step {
                Step32::Message(window, ..) | Step32::Capture(window) | Step32::Remove(window) => {
                    window
                }
            };
            let Ok(record) = self.windows().get(window) else {
                self.destructions.last_mut().unwrap().cursor += 1;
                continue;
            };
            let event = match step {
                Step32::Message(_, message, wparam, lparam) => {
                    if message == 0x0002 {
                        let mut data = self.state.record(window)?;
                        if data.destroying {
                            self.destructions.last_mut().unwrap().cursor += 1;
                            continue;
                        }
                        data.destroying = true;
                        data.style &= !0x1000_0000;
                        let mut description = record.description;
                        description.visible = false;
                        self.state
                            .windows_mut()
                            .update_description(window, description)?;
                        self.state.update(data)?;
                    }
                    Some(WindowMessage32 {
                        window,
                        procedure: record.description.procedure,
                        message,
                        wparam,
                        lparam,
                    })
                }
                Step32::Capture(_) => {
                    if self.windows().capture() == Some(window) {
                        self.state.windows_mut().set_capture(thread, None)?;
                        self.state.windows_mut().pop_message()
                    } else {
                        None
                    }
                }
                Step32::Remove(_) => {
                    let data = self.state.record(window)?;
                    let menu = (data.style & 0x4000_0000 == 0)
                        .then(|| {
                            crate::windows::user32::menus::MenuHandle32::try_from(data.menu).ok()
                        })
                        .flatten();
                    let menus = self.menus.prepare_retirement(menu)?;
                    self.state.remove_delivered(thread, window)?;
                    self.menus.commit_retirement(menus);
                    self.raw_input.retire_window(window.get());
                    self.timers.retire_window(window.get());
                    self.provenance.retire_window(window.get());
                    for dialog in &mut self.dialogs {
                        if dialog.owner == window.get() && dialog.result.is_none() {
                            dialog.result = Some((0, 1400));
                        }
                    }
                    None
                }
            };
            let plan = self.destructions.last_mut().unwrap();
            plan.cursor += 1;
            if let Some(event) = event {
                plan.event = Some(event);
                return self.destroy_outcome(token);
            }
        }
    }

    pub(super) fn destroy_completion_flags(&self, token: u64) -> Option<(bool, bool)> {
        let plan = self
            .destructions
            .last()
            .filter(|plan| plan.token == token)?;
        for step in &plan.steps[plan.cursor..] {
            match *step {
                Step32::Message(window, ..) if self.windows().get(window).is_ok() => return None,
                Step32::Capture(window) if self.windows().capture() == Some(window) => {
                    return Some((self.windows().focus().is_some(), false));
                }
                _ => {}
            }
        }
        None
    }
}
