use super::super::{
    User32CallContext32, User32Context32,
    callback::{Continuation32, CreateStage32},
};
use super::*;
use crate::{
    memory::{Access, AddressSpace},
    windows::user32::User32Api32,
};
impl User32Context32 {
    pub(in crate::process::windows::user32) fn hook_call(
        &mut self,
        api: User32Api32,
        a: &[u32],
        memory: &AddressSpace,
        c: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        use User32Api32::*;
        match api {
            SetWindowsHookExA => {
                let kind = a[0] as i32;
                if !(-1..=14).contains(&kind) {
                    return Err(User32Error32::Win32(1426));
                }
                if a[1] == 0 {
                    return Err(User32Error32::Win32(1427));
                }
                if kind != 5 || a[3] != c.thread_id {
                    return Err(User32Error32::Win32(120));
                }
                memory.check_span(GuestAddress(a[1]), 1, Access::Execute)?;
                self.hooks
                    .insert(c.thread_id, GuestAddress(a[1]))
                    .map(User32Outcome32::Return)
            }
            UnhookWindowsHookEx => {
                self.hooks.remove(a[0])?;
                Ok(User32Outcome32::Return(1))
            }
            CallNextHookEx => {
                let active =
                    self.pending
                        .iter()
                        .rev()
                        .find_map(|pending| match pending.continuation {
                            Continuation32::Hook(call)
                            | Continuation32::Create {
                                stage: CreateStage32::Hook(call),
                                ..
                            } => Some(call),
                            _ => None,
                        });
                let Some(active) = active.filter(|active| active.thread == c.thread_id) else {
                    return Ok(User32Outcome32::Return(0));
                };
                let Some(next) = self.hooks.next(c.thread_id, active.handle) else {
                    return Ok(User32Outcome32::Return(0));
                };
                self.start(Continuation32::Hook(HookCall32 {
                    handle: next.handle,
                    thread: next.thread,
                    procedure: next.procedure,
                    arguments: [a[1], a[2], a[3]],
                }))
            }
            _ => Err(User32Error32::InvalidRequest),
        }
    }
}
