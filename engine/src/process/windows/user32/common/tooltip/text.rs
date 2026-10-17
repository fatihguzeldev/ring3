use super::*;
use crate::process::windows::user32::{callback::Continuation32, memory::Scratch32};
#[derive(Clone, Copy)]
pub(super) struct TextTool32 {
    pub key: u64,
    pub revision: u64,
    pub owner: u32,
    pub id: u32,
    pub flags: u32,
    pub data: u32,
}
#[derive(Clone, Copy)]
enum Stage32 {
    Send,
    Read,
    Track,
    Finish,
}
#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct TooltipText32 {
    window: WindowHandle32,
    key: u64,
    revision: u64,
    owner: Option<WindowHandle32>,
    wide: bool,
    scratch: Scratch32,
    call: User32CallContext32,
    point: Point32,
    stage: Stage32,
    child: Option<u64>,
    completed: Option<u64>,
}
impl TooltipText32 {
    pub(in crate::process::windows::user32) fn scratch(self) -> Scratch32 {
        self.scratch
    }
}
impl User32Context32 {
    fn current_tooltip_text_tool(&self, plan: TooltipText32) -> Option<&Tool32> {
        let tool = self
            .tooltip(plan.window)
            .ok()?
            .tools
            .iter()
            .find(|tool| tool.key == plan.key && tool.revision == plan.revision)?;
        if tool.tab_owner != 0 {
            let owner = WindowHandle32::try_from(tool.tab_owner).ok()?;
            let CommonControlState32::Tab(tab) = self.state.common_state(owner).ok()? else {
                return None;
            };
            if tab.tooltip != plan.window.get()
                || !tab
                    .items
                    .get(tool.id as usize)
                    .is_some_and(|item| item.key == tool.tab_key)
            {
                return None;
            }
        }
        Some(tool)
    }
    pub(in crate::process::windows::user32::common::tooltip) fn begin_tooltip_text(
        &mut self,
        window: WindowHandle32,
        tool: TextTool32,
        ram: &mut AddressSpace,
        call: User32CallContext32,
        point: Point32,
    ) -> Result<User32Outcome32, HostError> {
        self.prepare_thread_slots().map_err(HostError::User32)?;
        if self.pending.len() + 2 > self.continuation_limit {
            return Err(HostError::User32(E::Capacity));
        }
        self.next_token
            .checked_add(2)
            .ok_or(HostError::User32(E::Capacity))?;
        let owner = WindowHandle32::try_from(tool.owner)
            .ok()
            .filter(|&owner| self.windows().get(owner).is_ok());
        let wide = owner
            .and_then(|owner| self.state.record(owner).ok())
            .is_none_or(|data| data.unicode);
        let mut bytes = [0u8; 188];
        for (index, value) in [
            window.get(),
            tool.id,
            if wide {
                (-530i32) as u32
            } else {
                (-520i32) as u32
            },
        ]
        .into_iter()
        .enumerate()
        {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        let flags = if wide { 180 } else { 100 };
        bytes[flags..flags + 4].copy_from_slice(&tool.flags.to_le_bytes());
        bytes[flags + 4..flags + 8].copy_from_slice(&tool.data.to_le_bytes());
        let scratch = Scratch32::allocate(
            ram,
            self.scratch_bounds,
            &bytes[..if wide { 188 } else { 108 }],
        )
        .map_err(HostError::User32)?;
        self.start(Continuation32::TooltipText(TooltipText32 {
            window,
            key: tool.key,
            revision: tool.revision,
            owner,
            wide,
            scratch,
            call,
            point,
            stage: Stage32::Send,
            child: None,
            completed: None,
        }))
        .map_err(HostError::User32)
    }
    fn tooltip_text(&self, token: u64) -> Result<TooltipText32, E> {
        let pending = self
            .pending
            .last()
            .filter(|p| p.token == token)
            .ok_or(E::InvalidToken)?;
        let Continuation32::TooltipText(plan) = pending.continuation else {
            return Err(E::InvalidToken);
        };
        Ok(plan)
    }
    fn publish_tooltip_text(&mut self, token: u64, plan: TooltipText32) {
        self.pending
            .iter_mut()
            .find(|p| p.token == token)
            .expect("same text owner")
            .continuation = Continuation32::TooltipText(plan);
    }
    pub(in crate::process::windows::user32) fn tooltip_text_wait(&self, token: u64) -> bool {
        self.pending.last().is_some_and(|p| {
            p.token == token && matches!(p.continuation, Continuation32::TooltipText(_))
        })
    }
    pub(in crate::process::windows::user32) fn pending_tooltip_text_message(
        &self,
        token: u64,
    ) -> Result<Option<[u32; 4]>, E> {
        let plan = self.tooltip_text(token)?;
        if !matches!(plan.stage, Stage32::Send) || plan.child.is_some() {
            return Ok(None);
        }
        let Some(tool) = self.current_tooltip_text_tool(plan) else {
            return Ok(None);
        };
        Ok(plan
            .owner
            .filter(|&owner| self.windows().get(owner).is_ok())
            .map(|owner| [owner.get(), 0x4e, tool.id, plan.scratch.address.0]))
    }
    fn accept_tooltip_text(&mut self, token: u64, ram: &AddressSpace) -> Result<(), E> {
        let mut plan = self.tooltip_text(token)?;
        plan.scratch.check(ram)?;
        let Some(index) = self.current_tooltip_text_tool(plan).and_then(|tool| {
            self.tooltip(plan.window)
                .ok()?
                .tools
                .iter()
                .position(|row| row.key == tool.key)
        }) else {
            plan.stage = Stage32::Finish;
            self.publish_tooltip_text(token, plan);
            return Ok(());
        };
        let pointer = memory::words::<1>(ram, plan.scratch.address.0 + 12)?[0];
        let flags_offset = if plan.wide { 180 } else { 100 };
        let [instance, flags] = memory::words::<2>(ram, plan.scratch.address.0 + flags_offset - 4)?;
        if instance != 0 {
            return Err(E::Win32(50));
        }
        let text = if pointer == 0 || pointer == plan.scratch.address.0 + 16 {
            let mut bytes = [0u8; 160];
            let length = if plan.wide { 160 } else { 80 };
            ram.read(
                GuestAddress(plan.scratch.address.0 + 16),
                &mut bytes[..length],
            )?;
            if plan.wide {
                let mut output = Vec::new();
                output.try_reserve_exact(80).map_err(|_| E::Allocation)?;
                for pair in bytes.chunks_exact(2) {
                    let unit = u16::from_le_bytes(pair.try_into().unwrap());
                    if unit == 0 {
                        break;
                    }
                    output.push(unit);
                }
                output
            } else {
                let end = bytes[..80].iter().position(|&byte| byte == 0).unwrap_or(80);
                plan.call
                    .ansi
                    .decode(&bytes[..end], false)
                    .map_err(|_| E::Win32(1113))?
            }
        } else {
            memory::string(ram, pointer, plan.wide, MAX_TEXT, plan.call.ansi)?
        };
        let old = &self.tooltip(plan.window)?.tools[index];
        let next = Tool32 {
            key: old.key,
            revision: old.revision,
            callback: old.callback && flags & 0x8000 == 0,
            callback_owner: old.callback_owner,
            tab_owner: old.tab_owner,
            tab_key: old.tab_key,
            window: old.window,
            id: old.id,
            flags: old.flags,
            rect: old.rect,
            data: old.data,
            text,
        };
        self.tooltip_mut(plan.window)?.prepare(&next, Some(index))?;
        self.tooltip_mut(plan.window)?.tools[index] = next;
        plan.stage = if self.tooltip(plan.window)?.tools[index].text.is_empty() {
            Stage32::Finish
        } else {
            Stage32::Track
        };
        self.publish_tooltip_text(token, plan);
        Ok(())
    }
    pub(in crate::process::windows::user32) fn resume_tooltip_text(
        &mut self,
        token: u64,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
    ) -> Result<User32Outcome32, HostError> {
        let mut plan = self.tooltip_text(token).map_err(HostError::User32)?;
        plan.scratch.check(ram).map_err(HostError::User32)?;
        if plan.child.is_some() {
            return Err(HostError::User32(E::InvalidToken));
        }
        if self.windows().get(plan.window).is_err() {
            plan.stage = Stage32::Finish;
        }
        let mut child_owner = None;
        let outcome = match plan.stage {
            Stage32::Send => {
                if let Some(args) = self
                    .pending_tooltip_text_message(token)
                    .map_err(HostError::User32)?
                {
                    self.send_message(&args, plan.call)
                        .map_err(HostError::User32)?
                } else {
                    plan.stage = Stage32::Finish;
                    User32Outcome32::Return(0)
                }
            }
            Stage32::Read => {
                self.accept_tooltip_text(token, ram)
                    .map_err(HostError::User32)?;
                return Ok(User32Outcome32::Wait {
                    continuation: token,
                });
            }
            Stage32::Track => {
                let valid = self.current_tooltip_text_tool(plan).is_some();
                if valid {
                    // tracking returns its position callback; await the tracking owner.
                    child_owner = Some(self.check_continuation().map_err(HostError::User32)?);
                    self.begin_tooltip_tracking_change(
                        plan.window,
                        Some(plan.key),
                        ram,
                        gdi,
                        plan.call,
                        Some(TrackingChange32::Resolved(plan.point)),
                    )?
                } else {
                    plan.stage = Stage32::Finish;
                    User32Outcome32::Return(0)
                }
            }
            Stage32::Finish => {
                plan.scratch.release(ram).map_err(HostError::User32)?;
                self.pending.pop();
                return Ok(User32Outcome32::Return(0));
            }
        };
        let child = match outcome {
            User32Outcome32::Callback(callback) => {
                Some(child_owner.unwrap_or(callback.continuation))
            }
            User32Outcome32::Wait { continuation } => Some(child_owner.unwrap_or(continuation)),
            _ => None,
        };
        if let Some(child) = child {
            plan.child = Some(child);
            plan.completed = None;
        } else if matches!(plan.stage, Stage32::Track) {
            plan.stage = Stage32::Finish;
        }
        self.publish_tooltip_text(token, plan);
        if child.is_some() {
            Ok(outcome)
        } else {
            Ok(User32Outcome32::Wait {
                continuation: token,
            })
        }
    }
    pub(in crate::process::windows::user32) fn complete_tooltip_text_child(
        &mut self,
        child: u64,
        outcome: User32Outcome32,
        ram: &AddressSpace,
    ) -> Result<User32Outcome32, E> {
        let User32Outcome32::Return(_) = outcome else {
            return Ok(outcome);
        };
        let Some(pending) = self.pending.last() else {
            return Ok(outcome);
        };
        let Continuation32::TooltipText(mut plan) = pending.continuation else {
            return Ok(outcome);
        };
        if plan.child != Some(child) {
            return Ok(outcome);
        }
        let token = pending.token;
        plan.child = None;
        plan.completed = Some(child);
        plan.stage = if matches!(plan.stage, Stage32::Send) {
            Stage32::Read
        } else {
            Stage32::Finish
        };
        self.publish_tooltip_text(token, plan);
        if matches!(plan.stage, Stage32::Read) {
            self.accept_tooltip_text(token, ram)?;
        }
        Ok(User32Outcome32::Wait {
            continuation: token,
        })
    }
    pub(in crate::process::windows::user32) fn retry_tooltip_text_completion(
        &self,
        child: u64,
    ) -> Option<User32Outcome32> {
        let pending = self.pending.last()?;
        let Continuation32::TooltipText(plan) = pending.continuation else {
            return None;
        };
        (plan.child.is_none() && plan.completed == Some(child)).then_some(User32Outcome32::Wait {
            continuation: pending.token,
        })
    }
}
