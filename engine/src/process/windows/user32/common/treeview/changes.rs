use super::*;
use crate::{
    process::windows::user32::{callback::Continuation32, memory::Scratch32},
    windows::user32::User32Api32,
};
#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) enum Change32 {
    Select(u32),
    Expand(u32, bool),
    Delete(u32),
    Ensure(u32),
}
#[derive(Clone, Copy)]
enum Stage32 {
    Focus,
    Prepare,
    Before,
    Apply,
    After,
    Scroll,
    Finish,
}
#[derive(Clone, Copy)]
enum Child32 {
    Focus,
    Before,
    After,
}
#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct TreeChange32 {
    window: WindowHandle32,
    parent: Option<WindowHandle32>,
    action: Change32,
    reason: u32,
    cause: u32,
    focus: bool,
    call: User32CallContext32,
    scratch: Scratch32,
    stage: Stage32,
    target: u32,
    revision: u64,
    result: u32,
    reveal: bool,
    expanded: bool,
    notify: bool,
    child: Option<(u64, Child32)>,
    completed: Option<u64>,
}
impl TreeChange32 {
    pub(in crate::process::windows::user32) fn scratch(self) -> Scratch32 {
        self.scratch
    }
}
impl User32Context32 {
    pub(in crate::process::windows::user32::common::treeview) fn begin_tree_change(
        &mut self,
        window: WindowHandle32,
        action: Change32,
        reason: u32,
        focus: bool,
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        let tree = self.tree(window)?;
        let valid = match action {
            Change32::Select(id) => id == 0 || tree.item(id).is_some(),
            Change32::Delete(id) => id == ROOT || tree.item(id).is_some(),
            Change32::Expand(id, _) | Change32::Ensure(id) => tree.item(id).is_some(),
        };
        if !valid {
            return Ok(User32Outcome32::Return(0));
        }
        self.prepare_thread_slots()?;
        if self.pending.len() + 2 > self.continuation_limit {
            return Err(E::Capacity);
        }
        self.next_token.checked_add(2).ok_or(E::Capacity)?;
        let parent = self.state.record(window)?.parent;
        let scratch = Scratch32::allocate(ram, self.scratch_bounds, &[0; 104])?;
        self.start(Continuation32::TreeView(TreeChange32 {
            window,
            parent,
            action,
            reason,
            cause: reason,
            focus,
            call,
            scratch,
            stage: Stage32::Focus,
            target: 0,
            revision: 0,
            result: 0,
            reveal: false,
            expanded: false,
            notify: true,
            child: None,
            completed: None,
        }))
    }
    fn tree_change(&self, token: u64) -> Result<TreeChange32, E> {
        let pending = self
            .pending
            .last()
            .filter(|p| p.token == token)
            .ok_or(E::InvalidToken)?;
        let Continuation32::TreeView(plan) = pending.continuation else {
            return Err(E::InvalidToken);
        };
        Ok(plan)
    }
    fn publish_tree_change(&mut self, token: u64, plan: TreeChange32) {
        self.pending
            .iter_mut()
            .find(|p| p.token == token)
            .expect("same tree operation")
            .continuation = Continuation32::TreeView(plan);
    }
    pub(in crate::process::windows::user32) fn tree_change_wait(&self, token: u64) -> bool {
        self.pending.last().is_some_and(|p| {
            p.token == token && matches!(p.continuation, Continuation32::TreeView(_))
        })
    }
    pub(in crate::process::windows::user32) fn preview_tree_input_flags(
        &self,
        token: u64,
    ) -> Result<Option<(bool, bool)>, E> {
        if !self.tree_change_wait(token) {
            return Ok(None);
        }
        let plan = self.tree_change(token)?;
        if matches!(plan.stage, Stage32::Focus)
            && plan.focus
            && self.windows().get(plan.window).is_ok()
        {
            self.windows()
                .check_set_focus(plan.call.thread_id, Some(plan.window))?;
            return Ok(Some((true, self.windows().capture().is_some())));
        }
        Ok(None)
    }
    pub(in crate::process::windows::user32) fn pending_tree_message(
        &self,
        token: u64,
    ) -> Result<Option<[u32; 4]>, E> {
        let plan = self.tree_change(token)?;
        if plan.child.is_some()
            || !plan.notify
            || !matches!(plan.stage, Stage32::Before | Stage32::After)
            || self.windows().get(plan.window).is_err()
        {
            return Ok(None);
        }
        Ok(plan
            .parent
            .filter(|&parent| self.windows().get(parent).is_ok())
            .map(|parent| {
                [
                    parent.get(),
                    0x4e,
                    self.state.record(plan.window).expect("live tree").menu,
                    plan.scratch.address.0,
                ]
            }))
    }
    fn tree_notification(
        &self,
        plan: TreeChange32,
        code: i32,
        old: u32,
        new: u32,
    ) -> Result<[u8; 104], E> {
        let tree = self.tree(plan.window)?;
        let mut words = [0u32; 26];
        words[..4].copy_from_slice(&[
            plan.window.get(),
            self.state.record(plan.window)?.menu,
            code as u32,
            plan.reason,
        ]);
        for (offset, id) in [(4, old), (14, new)] {
            if let Some(item) = tree.item(id) {
                words[offset] = 0x1c;
                words[offset + 1] = id;
                words[offset + 2] = tree.state(item);
                words[offset + 9] = item.data;
            }
        }
        let mut bytes = [0u8; 104];
        for (out, value) in bytes.chunks_exact_mut(4).zip(words) {
            out.copy_from_slice(&value.to_le_bytes());
        }
        Ok(bytes)
    }
    fn tree_codes(&self, window: WindowHandle32, selection: bool, after: bool) -> Result<i32, E> {
        Ok(if selection {
            if self.tree(window)?.unicode {
                -450
            } else {
                -401
            }
        } else {
            if self.tree(window)?.unicode {
                -454
            } else {
                -405
            }
        } - i32::from(after))
    }
    pub(in crate::process::windows::user32) fn resume_tree_change(
        &mut self,
        token: u64,
        ram: &mut AddressSpace,
        gdi: &GdiRuntime32,
    ) -> Result<User32Outcome32, E> {
        let mut plan = self.tree_change(token)?;
        plan.scratch.check(ram)?;
        if plan.child.is_some() {
            return Err(E::InvalidToken);
        }
        if self.windows().get(plan.window).is_err() {
            plan.stage = Stage32::Finish;
        }
        match plan.stage {
            Stage32::Focus => {
                plan.stage = Stage32::Prepare;
                if plan.focus && self.windows().focus() != Some(plan.window) {
                    let outcome = self.prepare_arguments(
                        User32Api32::SetFocus,
                        &[plan.window.get()],
                        ram,
                        plan.call,
                    )?;
                    if let Some(child) = child_token(outcome) {
                        plan.child = Some((child, Child32::Focus));
                        plan.completed = None;
                        self.publish_tree_change(token, plan);
                        return Ok(outcome);
                    }
                }
            }
            Stage32::Prepare => {
                plan.reveal = false;
                plan.notify = true;
                let tree = self.tree(plan.window)?;
                let reveal = match plan.action {
                    Change32::Select(id) | Change32::Ensure(id) if id != 0 => {
                        let mut current = id;
                        let mut collapsed = 0;
                        while let Some(item) = tree.item(current) {
                            if item.parent == 0 {
                                break;
                            }
                            current = item.parent;
                            if tree
                                .item(current)
                                .is_some_and(|item| item.state & 0x20 == 0)
                            {
                                collapsed = current;
                            }
                        }
                        (collapsed != 0).then_some(collapsed)
                    }
                    _ => None,
                };
                let (target, old, new, code, next) = if let Some(id) = reveal {
                    plan.reveal = true;
                    plan.reason = 2;
                    (
                        id,
                        0,
                        id,
                        self.tree_codes(plan.window, false, false)?,
                        Stage32::Before,
                    )
                } else {
                    match plan.action {
                        Change32::Select(id) => {
                            plan.reason = plan.cause;
                            if id != 0 && tree.item(id).is_none() {
                                plan.result = 0;
                                plan.stage = Stage32::Finish;
                                self.publish_tree_change(token, plan);
                                return Ok(User32Outcome32::Wait {
                                    continuation: token,
                                });
                            }
                            if tree.selected == id {
                                plan.result = 1;
                                plan.stage = Stage32::Scroll;
                                self.publish_tree_change(token, plan);
                                return Ok(User32Outcome32::Wait {
                                    continuation: token,
                                });
                            }
                            (
                                id,
                                tree.selected,
                                id,
                                self.tree_codes(plan.window, true, false)?,
                                Stage32::Before,
                            )
                        }
                        Change32::Expand(id, expanded) => {
                            if !tree.has_children(id) {
                                plan.result = 0;
                                plan.stage = Stage32::Finish;
                                self.publish_tree_change(token, plan);
                                return Ok(User32Outcome32::Wait {
                                    continuation: token,
                                });
                            }
                            let current = tree.item(id).expect("same expansion target");
                            if (current.state & 0x20 != 0) == expanded {
                                plan.result = u32::from(expanded);
                                plan.stage = Stage32::Finish;
                                self.publish_tree_change(token, plan);
                                return Ok(User32Outcome32::Wait {
                                    continuation: token,
                                });
                            }
                            plan.notify = plan.cause != 0 || current.state & 0x40 == 0;
                            plan.reason = if expanded { 2 } else { 1 };
                            (
                                id,
                                0,
                                id,
                                self.tree_codes(plan.window, false, false)?,
                                if plan.notify {
                                    Stage32::Before
                                } else {
                                    Stage32::Apply
                                },
                            )
                        }
                        Change32::Delete(root) => {
                            let id = tree.deletion_leaf(root);
                            if id == 0 {
                                plan.result = 1;
                                plan.stage = Stage32::Finish;
                                self.publish_tree_change(token, plan);
                                return Ok(User32Outcome32::Wait {
                                    continuation: token,
                                });
                            }
                            (
                                id,
                                id,
                                0,
                                if tree.unicode { -458 } else { -409 },
                                Stage32::Apply,
                            )
                        }
                        Change32::Ensure(_) => {
                            plan.stage = Stage32::Scroll;
                            self.publish_tree_change(token, plan);
                            return Ok(User32Outcome32::Wait {
                                continuation: token,
                            });
                        }
                    }
                };
                let revision = tree.item(target).map_or(0, |item| item.revision);
                let bytes = self.tree_notification(plan, code, old, new)?;
                ram.write(GuestAddress(plan.scratch.address.0), &bytes)?;
                plan.target = target;
                plan.revision = revision;
                plan.stage = next;
            }
            Stage32::Before | Stage32::After => {
                if let Some(message) = self.pending_tree_message(token)? {
                    let kind = if matches!(plan.stage, Stage32::Before) {
                        Child32::Before
                    } else {
                        Child32::After
                    };
                    let outcome = self.send_message(&message, plan.call)?;
                    let child = child_token(outcome).ok_or(E::InvalidRequest)?;
                    plan.child = Some((child, kind));
                    plan.completed = None;
                    self.publish_tree_change(token, plan);
                    return Ok(outcome);
                }
                plan.stage = if matches!(plan.stage, Stage32::Before) {
                    Stage32::Apply
                } else if plan.reveal || matches!(plan.action, Change32::Delete(_)) {
                    Stage32::Prepare
                } else if matches!(plan.action, Change32::Select(_)) {
                    Stage32::Scroll
                } else {
                    Stage32::Finish
                };
            }
            Stage32::Apply => {
                if plan.target != 0
                    && self
                        .tree(plan.window)?
                        .item(plan.target)
                        .is_none_or(|item| item.revision != plan.revision)
                {
                    plan.stage = Stage32::Finish;
                } else {
                    self.state.check_paint_change()?;
                    let (code, old, new) = if plan.reveal {
                        (self.tree_codes(plan.window, false, true)?, 0, plan.target)
                    } else {
                        match plan.action {
                            Change32::Select(id) => (
                                self.tree_codes(plan.window, true, true)?,
                                self.tree(plan.window)?.selected,
                                id,
                            ),
                            Change32::Expand(id, _) | Change32::Ensure(id) => (
                                self.tree_codes(plan.window, false, true)?,
                                0,
                                if matches!(plan.action, Change32::Ensure(_)) {
                                    plan.target
                                } else {
                                    id
                                },
                            ),
                            Change32::Delete(_) => (
                                if self.tree(plan.window)?.unicode {
                                    -458
                                } else {
                                    -409
                                },
                                plan.target,
                                0,
                            ),
                        }
                    };
                    if !matches!(plan.action, Change32::Delete(_)) {
                        let mut bytes = self.tree_notification(plan, code, old, new)?;
                        let state = u32::from_le_bytes(bytes[64..68].try_into().unwrap());
                        let next_state = if plan.reveal {
                            state | 0x60
                        } else {
                            match plan.action {
                                Change32::Select(_) => state | 2,
                                Change32::Expand(_, true) | Change32::Ensure(_) => state | 0x60,
                                Change32::Expand(_, false) => state & !0x20,
                                Change32::Delete(_) => state,
                            }
                        };
                        bytes[64..68].copy_from_slice(&next_state.to_le_bytes());
                        if !plan.reveal && matches!(plan.action, Change32::Select(_)) {
                            let old_state = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
                            bytes[24..28].copy_from_slice(&(old_state & !2).to_le_bytes());
                        }
                        ram.write(GuestAddress(plan.scratch.address.0), &bytes)?;
                    }
                    let tree = self.tree_mut(plan.window)?;
                    if plan.reveal {
                        tree.item_mut(plan.target)
                            .expect("same ancestor target")
                            .state |= 0x60;
                        plan.expanded = true;
                    } else {
                        match plan.action {
                            Change32::Select(id) => tree.selected = id,
                            Change32::Expand(id, expanded) => {
                                let item = tree.item_mut(id).expect("same expansion target");
                                if expanded {
                                    item.state |= 0x60;
                                } else {
                                    item.state &= !0x20;
                                }
                            }
                            Change32::Ensure(_) => {
                                tree.item_mut(plan.target)
                                    .expect("same expansion target")
                                    .state |= 0x60
                            }
                            Change32::Delete(_) => tree.remove_leaf(plan.target),
                        }
                    }
                    self.tree_changed(plan.window);
                    plan.result = 1;
                    plan.stage = Stage32::After;
                }
            }
            Stage32::Scroll => {
                let id = match plan.action {
                    Change32::Select(id) | Change32::Ensure(id) => id,
                    _ => 0,
                };
                let mut scrolled = false;
                if id != 0 {
                    let tree = self.tree(plan.window)?;
                    let ids = tree.visible_ids()?;
                    if let Some(index) = ids.iter().position(|&value| value == id) {
                        let height = self
                            .tree_row_height(plan.window, gdi)
                            .map_err(|error| E::Win32(error.win32()))?;
                        let rows = (self.windows().get(plan.window)?.description.extent.height
                            / height as u32)
                            .max(1) as usize;
                        let first = ids
                            .iter()
                            .position(|&value| value == tree.first)
                            .unwrap_or(0);
                        let next = if index < first {
                            index
                        } else if index >= first.saturating_add(rows) {
                            index.saturating_sub(rows - 1)
                        } else {
                            first
                        };
                        let next = next.min(ids.len().saturating_sub(rows));
                        if first != next {
                            self.state.check_paint_change()?;
                            self.tree_mut(plan.window)?.first = ids[next];
                            self.tree_changed(plan.window);
                            scrolled = true;
                        }
                    }
                }
                if matches!(plan.action, Change32::Ensure(_)) {
                    plan.result = u32::from(scrolled && !plan.expanded);
                }
                plan.stage = Stage32::Finish;
            }
            Stage32::Finish => {
                plan.scratch.release(ram)?;
                self.pending.pop();
                return Ok(User32Outcome32::Return(plan.result));
            }
        }
        self.publish_tree_change(token, plan);
        Ok(User32Outcome32::Wait {
            continuation: token,
        })
    }
    pub(in crate::process::windows::user32) fn complete_tree_child(
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
        let Continuation32::TreeView(mut plan) = pending.continuation else {
            return Ok(outcome);
        };
        let Some((expected, kind)) = plan.child else {
            return Ok(outcome);
        };
        if expected != child {
            return Ok(outcome);
        }
        let token = pending.token;
        plan.child = None;
        plan.completed = Some(child);
        plan.stage = match kind {
            Child32::Focus => Stage32::Prepare,
            Child32::Before if result == 0 => Stage32::Apply,
            Child32::Before => {
                plan.result = 0;
                Stage32::Finish
            }
            Child32::After => {
                if plan.reveal || matches!(plan.action, Change32::Delete(_)) {
                    Stage32::Prepare
                } else if matches!(plan.action, Change32::Select(_)) {
                    Stage32::Scroll
                } else {
                    Stage32::Finish
                }
            }
        };
        self.publish_tree_change(token, plan);
        Ok(User32Outcome32::Wait {
            continuation: token,
        })
    }
    pub(in crate::process::windows::user32) fn retry_tree_completion(
        &self,
        child: u64,
    ) -> Option<User32Outcome32> {
        let p = self.pending.last()?;
        let Continuation32::TreeView(plan) = p.continuation else {
            return None;
        };
        (plan.child.is_none() && plan.completed == Some(child)).then_some(User32Outcome32::Wait {
            continuation: p.token,
        })
    }
}
fn child_token(outcome: User32Outcome32) -> Option<u64> {
    match outcome {
        User32Outcome32::Wait { continuation } => Some(continuation),
        User32Outcome32::Callback(callback) => Some(callback.continuation),
        _ => None,
    }
}
