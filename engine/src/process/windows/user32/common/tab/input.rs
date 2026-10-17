use super::*;
use crate::process::windows::user32::{callback::Continuation32, memory::Scratch32};
use crate::windows::user32::User32Api32;
#[derive(Clone, Copy)]
enum Stage32 {
    Focus,
    Changing,
    Apply,
    Changed,
    Finish,
}
#[derive(Clone, Copy)]
enum Child32 {
    Focus,
    Changing,
    Changed,
}
#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct TabSelect32 {
    window: WindowHandle32,
    parent: Option<WindowHandle32>,
    key: u64,
    call: User32CallContext32,
    scratch: Scratch32,
    stage: Stage32,
    focus: bool,
    change: bool,
    child: Option<(u64, Child32)>,
    completed: Option<u64>,
}
impl TabSelect32 {
    pub(in crate::process::windows::user32) fn scratch(self) -> Scratch32 {
        self.scratch
    }
}
impl User32Context32 {
    pub(in crate::process::windows::user32::common::tab) fn begin_tab_selection(
        &mut self,
        window: WindowHandle32,
        index: usize,
        focus: bool,
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        let tab = self.tab(window)?;
        let key = tab.items[index].key;
        let change = tab.selected != index as i32;
        if tab.selected == index as i32 && !focus {
            return Ok(User32Outcome32::Return(0));
        }
        self.prepare_thread_slots()?;
        if self.pending.len() + 2 > self.continuation_limit {
            return Err(E::Capacity);
        }
        self.next_token.checked_add(2).ok_or(E::Capacity)?;
        let record = self.state.record(window)?;
        let parent = record.parent;
        let id = record.menu;
        let mut bytes = [0; 12];
        for (out, value) in bytes
            .chunks_exact_mut(4)
            .zip([window.get(), id, (-552i32) as u32])
        {
            out.copy_from_slice(&value.to_le_bytes());
        }
        let scratch = Scratch32::allocate(ram, self.scratch_bounds, &bytes)?;
        self.start(Continuation32::TabSelection(TabSelect32 {
            window,
            parent,
            key,
            call,
            scratch,
            stage: Stage32::Focus,
            focus,
            change,
            child: None,
            completed: None,
        }))
    }
    fn tab_selection(&self, token: u64) -> Result<TabSelect32, E> {
        let pending = self
            .pending
            .last()
            .filter(|pending| pending.token == token)
            .ok_or(E::InvalidToken)?;
        let Continuation32::TabSelection(call) = pending.continuation else {
            return Err(E::InvalidToken);
        };
        Ok(call)
    }
    fn publish_tab_selection(&mut self, token: u64, call: TabSelect32) {
        self.pending
            .iter_mut()
            .find(|pending| pending.token == token)
            .expect("same tab selection owner")
            .continuation = Continuation32::TabSelection(call);
    }
    pub(in crate::process::windows::user32) fn tab_selection_wait(&self, token: u64) -> bool {
        self.pending.last().is_some_and(|pending| {
            pending.token == token
                && matches!(pending.continuation, Continuation32::TabSelection(_))
        })
    }
    pub(in crate::process::windows::user32) fn preview_tab_selection_input_flags(
        &self,
        token: u64,
    ) -> Result<Option<(bool, bool)>, E> {
        if !self.tab_selection_wait(token) {
            return Ok(None);
        }
        let call = self.tab_selection(token)?;
        if !matches!(call.stage, Stage32::Focus)
            || !call.focus
            || self.windows().get(call.window).is_err()
        {
            return Ok(None);
        }
        self.windows()
            .check_set_focus(call.call.thread_id, Some(call.window))?;
        Ok(Some((true, self.windows().capture().is_some())))
    }
    pub(in crate::process::windows::user32) fn pending_tab_selection_message(
        &self,
        token: u64,
    ) -> Result<Option<[u32; 4]>, E> {
        let call = self.tab_selection(token)?;
        if call.child.is_some() {
            return Err(E::InvalidToken);
        }
        if self.windows().get(call.window).is_err() {
            return Ok(None);
        }
        if !matches!(call.stage, Stage32::Changing | Stage32::Changed) {
            return Ok(None);
        }
        let Some(parent) = call.parent else {
            return Ok(None);
        };
        if self.windows().get(parent).is_err() {
            return Ok(None);
        }
        Ok(Some([
            parent.get(),
            0x4e,
            self.state.record(call.window)?.menu,
            call.scratch.address.0,
        ]))
    }
    pub(in crate::process::windows::user32) fn resume_tab_selection(
        &mut self,
        token: u64,
        ram: &mut AddressSpace,
    ) -> Result<User32Outcome32, E> {
        let mut call = self.tab_selection(token)?;
        call.scratch.check(ram)?;
        if call.child.is_some() {
            return Err(E::InvalidToken);
        }
        if self.windows().get(call.window).is_err() {
            call.stage = Stage32::Finish;
        }
        match call.stage {
            Stage32::Focus => {
                call.stage = if call.change {
                    Stage32::Changing
                } else {
                    Stage32::Finish
                };
                if call.focus && self.windows().focus() != Some(call.window) {
                    let outcome = self.prepare_arguments(
                        User32Api32::SetFocus,
                        &[call.window.get()],
                        ram,
                        call.call,
                    )?;
                    if let Some(child) = child_token(outcome) {
                        call.child = Some((child, Child32::Focus));
                        call.completed = None;
                        self.publish_tab_selection(token, call);
                        return Ok(outcome);
                    }
                }
            }
            Stage32::Changing | Stage32::Changed => {
                if let Some(message) = self.pending_tab_selection_message(token)? {
                    let kind = if matches!(call.stage, Stage32::Changing) {
                        Child32::Changing
                    } else {
                        Child32::Changed
                    };
                    let outcome = self.send_message(&message, call.call)?;
                    let child = child_token(outcome).ok_or(E::InvalidRequest)?;
                    call.child = Some((child, kind));
                    call.completed = None;
                    self.publish_tab_selection(token, call);
                    return Ok(outcome);
                }
                call.stage = if matches!(call.stage, Stage32::Changing) {
                    Stage32::Apply
                } else {
                    Stage32::Finish
                };
            }
            Stage32::Apply => {
                let target = self
                    .tab(call.window)?
                    .items
                    .iter()
                    .position(|item| item.key == call.key);
                if let Some(index) = target {
                    self.state.check_paint_change()?;
                    ram.write(
                        GuestAddress(call.scratch.address.0 + 8),
                        &(-551i32).to_le_bytes(),
                    )?;
                    self.tab_mut(call.window)?.select(index as i32);
                    self.tab_changed(call.window);
                    call.stage = Stage32::Changed;
                } else {
                    call.stage = Stage32::Finish;
                }
            }
            Stage32::Finish => {
                call.scratch.release(ram)?;
                self.pending.pop();
                return Ok(User32Outcome32::Return(0));
            }
        }
        self.publish_tab_selection(token, call);
        Ok(User32Outcome32::Wait {
            continuation: token,
        })
    }
    pub(in crate::process::windows::user32) fn complete_tab_selection_child(
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
        let Continuation32::TabSelection(mut call) = pending.continuation else {
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
        call.stage = match kind {
            Child32::Focus => {
                if call.change {
                    Stage32::Changing
                } else {
                    Stage32::Finish
                }
            }
            Child32::Changing if result == 0 => Stage32::Apply,
            Child32::Changing | Child32::Changed => Stage32::Finish,
        };
        self.publish_tab_selection(token, call);
        Ok(User32Outcome32::Wait {
            continuation: token,
        })
    }
    pub(in crate::process::windows::user32) fn retry_tab_selection_completion(
        &self,
        child: u64,
    ) -> Option<User32Outcome32> {
        let pending = self.pending.last()?;
        let Continuation32::TabSelection(call) = pending.continuation else {
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
