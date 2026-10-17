use super::*;
use crate::{
    process::windows::user32::callback::Continuation32,
    windows::{
        comctl32::{
            CommonControlKind32,
            tooltip::{MAX_TOOLS, Tool32},
        },
        user32::User32Api32,
        window::WindowHandle32,
    },
};
#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct TabTooltip32 {
    window: WindowHandle32,
    create: bool,
    tip: u32,
    result: u32,
    call: User32CallContext32,
    child: Option<u64>,
    completed: Option<u64>,
    finished: bool,
}
impl User32Context32 {
    pub(in crate::process::windows::user32) fn tab_tooltip_window(
        &self,
        source: u32,
    ) -> Option<WindowHandle32> {
        let source = WindowHandle32::try_from(source).ok()?;
        let CommonControlState32::Tab(tab) = self.state.common_state(source).ok()? else {
            return None;
        };
        WindowHandle32::try_from(tab.tooltip)
            .ok()
            .filter(|&window| {
                matches!(
                    self.state.common_state(window),
                    Ok(CommonControlState32::Tooltip(_))
                )
            })
    }
    pub(in crate::process::windows::user32::common::tab) fn begin_tab_tooltip(
        &mut self,
        window: WindowHandle32,
        create: bool,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        if create && self.tab_tooltip_window(window.get()).is_some() {
            return Ok(User32Outcome32::Return(0));
        }
        self.prepare_thread_slots()?;
        if self.pending.len() + 2 > self.continuation_limit {
            return Err(E::Capacity);
        }
        self.next_token.checked_add(2).ok_or(E::Capacity)?;
        self.start(Continuation32::TabTooltip(TabTooltip32 {
            window,
            create,
            tip: 0,
            result: 0,
            call,
            child: None,
            completed: None,
            finished: false,
        }))
    }
    pub(in crate::process::windows::user32::common::tab) fn set_tab_tooltip(
        &mut self,
        window: WindowHandle32,
        value: u32,
        call: User32CallContext32,
    ) -> Result<(), E> {
        if value != 0 {
            let tip = self.owned(value, call.thread_id)?;
            if !matches!(
                self.state.common_state(tip)?,
                CommonControlState32::Tooltip(_)
            ) {
                return Err(E::Win32(1407));
            }
        }
        let old = self.tab(window)?.tooltip;
        if old == value {
            return Ok(());
        }
        if let Ok(old) = WindowHandle32::try_from(old)
            && let Ok(CommonControlState32::Tooltip(tip)) = self.state.common_state_mut(old)
        {
            tip.tools.retain(|tool| tool.tab_owner != window.get());
        }
        let tab = self.tab_mut(window)?;
        tab.tooltip = value;
        tab.tooltip_owned = false;
        Ok(())
    }
    pub(in crate::process::windows::user32) fn tab_tooltip_wait(&self, token: u64) -> bool {
        self.pending.last().is_some_and(|p| {
            p.token == token && matches!(p.continuation, Continuation32::TabTooltip(_))
        })
    }
    pub(in crate::process::windows::user32) fn resume_tab_tooltip(
        &mut self,
        token: u64,
        ram: &mut AddressSpace,
    ) -> Result<User32Outcome32, E> {
        let pending = self
            .pending
            .last()
            .filter(|p| p.token == token)
            .ok_or(E::InvalidToken)?;
        let Continuation32::TabTooltip(mut plan) = pending.continuation else {
            return Err(E::InvalidToken);
        };
        if plan.child.is_some() {
            return Err(E::InvalidToken);
        }
        if plan.finished || (self.windows().get(plan.window).is_err() && plan.tip == 0) {
            self.pending.pop();
            return Ok(User32Outcome32::Return(plan.result));
        }
        let outcome = if plan.create {
            let atom = self
                .state
                .common_atom(CommonControlKind32::Tooltip)
                .ok_or(E::Win32(1407))?;
            self.create_window(
                if self.state.record(plan.window)?.unicode {
                    User32Api32::CreateWindowExW
                } else {
                    User32Api32::CreateWindowExA
                },
                &[
                    0x08000008,
                    u32::from(atom),
                    0,
                    0x80000003,
                    0,
                    0,
                    0,
                    0,
                    plan.window.get(),
                    0,
                    plan.call.instance,
                    0,
                ],
                ram,
                plan.call,
            )?
        } else {
            let value = if plan.tip != 0 {
                plan.tip
            } else {
                self.tab(plan.window)?.tooltip
            };
            if let Ok(tip) = WindowHandle32::try_from(value)
                && self.windows().get(tip).is_ok()
            {
                self.destroy_window(tip.get(), plan.call)?
            } else {
                plan.finished = true;
                User32Outcome32::Return(0)
            }
        };
        let child = match outcome {
            User32Outcome32::Callback(callback) => Some(callback.continuation),
            User32Outcome32::Wait { continuation } => Some(continuation),
            _ => None,
        };
        if let Some(child) = child {
            plan.child = Some(child);
            plan.completed = None;
        } else {
            plan.finished = true;
        }
        self.pending
            .iter_mut()
            .find(|p| p.token == token)
            .expect("same tab tooltip owner")
            .continuation = Continuation32::TabTooltip(plan);
        if child.is_some() {
            Ok(outcome)
        } else {
            Ok(User32Outcome32::Wait {
                continuation: token,
            })
        }
    }
    pub(in crate::process::windows::user32) fn complete_tab_tooltip_child(
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
        let Continuation32::TabTooltip(mut plan) = pending.continuation else {
            return Ok(outcome);
        };
        if plan.child != Some(child) {
            return Ok(outcome);
        }
        let token = pending.token;
        let live = self.tab_mut(plan.window).is_ok();
        if let Ok(tab) = self.tab_mut(plan.window) {
            tab.tooltip = if plan.create { result } else { 0 };
            tab.tooltip_owned = plan.create && result != 0;
        }
        plan.child = None;
        plan.completed = Some(child);
        if plan.create && result != 0 && !live {
            plan.tip = result;
            plan.create = false;
            plan.finished = false;
        } else {
            plan.finished = true;
            if plan.create && result == 0 {
                plan.result = u32::MAX;
            }
        }
        self.pending
            .last_mut()
            .expect("same tab tooltip parent")
            .continuation = Continuation32::TabTooltip(plan);
        Ok(User32Outcome32::Wait {
            continuation: token,
        })
    }
    pub(in crate::process::windows::user32) fn retry_tab_tooltip_completion(
        &self,
        child: u64,
    ) -> Option<User32Outcome32> {
        let pending = self.pending.last()?;
        let Continuation32::TabTooltip(plan) = pending.continuation else {
            return None;
        };
        (plan.child.is_none() && plan.completed == Some(child)).then_some(User32Outcome32::Wait {
            continuation: pending.token,
        })
    }
    pub(in crate::process::windows::user32) fn sync_tab_tooltip(
        &mut self,
        tip_window: WindowHandle32,
        source: u32,
        ram: &AddressSpace,
        gdi: &mut GdiRuntime32,
        thread: u32,
    ) -> Result<(), HostError> {
        if self.tab_tooltip_window(source) != Some(tip_window) {
            return Ok(());
        }
        let window = WindowHandle32::try_from(source)
            .map_err(E::from)
            .map_err(HostError::User32)?;
        let layout = self.tab_layout(window, ram, gdi, thread)?;
        let tab = self.tab(window).map_err(HostError::User32)?;
        let CommonControlState32::Tooltip(tip) = self
            .state
            .common_state(tip_window)
            .map_err(HostError::User32)?
        else {
            return Err(HostError::InvalidRequest);
        };
        let retained = tip
            .tools
            .iter()
            .filter(|tool| tool.tab_owner != source)
            .count();
        if retained + tab.items.len() > MAX_TOOLS {
            return Err(HostError::User32(E::Capacity));
        }
        let mut desired = Vec::new();
        desired
            .try_reserve_exact(retained + tab.items.len())
            .map_err(|_| HostError::User32(E::Allocation))?;
        let parent = self
            .state
            .record(window)
            .map_err(HostError::User32)?
            .parent
            .map_or(0, WindowHandle32::get);
        let mut new_keys = 0;
        for (index, (item, rect)) in tab.items.iter().zip(layout.items).enumerate() {
            let old = tip
                .tools
                .iter()
                .find(|tool| tool.tab_owner == source && tool.tab_key == item.key);
            let (key, revision) = if let Some(old) = old {
                (
                    old.key,
                    old.revision
                        .checked_add(u64::from(old.id != index as u32 || old.rect != rect))
                        .ok_or(HostError::User32(E::Capacity))?,
                )
            } else {
                new_keys += 1;
                (0, 1)
            };
            desired.push(Tool32 {
                key,
                revision,
                window: source,
                id: index as u32,
                flags: 0x10,
                rect,
                data: 0,
                text: Vec::new(),
                callback: true,
                callback_owner: parent,
                tab_owner: source,
                tab_key: item.key,
            });
        }
        tip.prepare_keys(new_keys).map_err(HostError::User32)?;
        let CommonControlState32::Tooltip(tip) = self
            .state
            .common_state_mut(tip_window)
            .expect("same tooltip")
        else {
            unreachable!();
        };
        let old = std::mem::take(&mut tip.tools);
        for old in old {
            if old.tab_owner != source {
                desired.push(old);
            } else if let Some(next) = desired
                .iter_mut()
                .find(|next| next.key != 0 && next.key == old.key)
            {
                next.text = old.text;
                next.callback = old.callback;
            }
        }
        for tool in &mut desired {
            if tool.key == 0 {
                tool.key = tip.issue_key();
            }
        }
        tip.tools = desired;
        Ok(())
    }
}
