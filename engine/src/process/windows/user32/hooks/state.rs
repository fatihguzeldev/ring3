use super::HookKind32;
use crate::{memory::GuestAddress, windows::user32::User32Error32};

#[derive(Clone, Copy)]
pub(super) struct Hook32 {
    pub kind: HookKind32,
    pub handle: u32,
    pub thread: u32,
    pub procedure: GuestAddress,
}

#[derive(Default)]
pub(in crate::process::windows::user32) struct Hooks32 {
    entries: Vec<Hook32>,
    next: u32,
}

impl Hooks32 {
    pub(super) fn insert(
        &mut self,
        kind: HookKind32,
        thread: u32,
        procedure: GuestAddress,
    ) -> Result<u32, User32Error32> {
        if self.entries.len() == 256 {
            return Err(User32Error32::Capacity);
        }
        let serial = self
            .next
            .checked_add(1)
            .filter(|id| *id <= 0x0fff_ffff)
            .ok_or(User32Error32::Capacity)?;
        self.entries
            .try_reserve(1)
            .map_err(|_| User32Error32::Allocation)?;
        let handle = 0x4000_0000 | serial;
        self.entries.push(Hook32 {
            kind,
            handle,
            thread,
            procedure,
        });
        self.next = serial;
        Ok(handle)
    }
    pub(super) fn remove(&mut self, handle: u32) -> Result<(), User32Error32> {
        let index = self
            .entries
            .iter()
            .position(|hook| hook.handle == handle)
            .ok_or(User32Error32::Win32(1404))?;
        self.entries.remove(index);
        Ok(())
    }
    pub(in crate::process::windows::user32) fn first_for_thread(&self, thread: u32) -> bool {
        self.first(thread, HookKind32::Cbt).is_some()
    }
    pub(super) fn first(&self, thread: u32, kind: HookKind32) -> Option<Hook32> {
        self.entries
            .iter()
            .rev()
            .find(|hook| hook.thread == thread && hook.kind == kind)
            .copied()
    }
    pub(super) fn next(&self, thread: u32, kind: HookKind32, after: u32) -> Option<Hook32> {
        self.entries
            .iter()
            .rev()
            .find(|hook| hook.thread == thread && hook.kind == kind && hook.handle < after)
            .copied()
    }
    pub(in crate::process::windows::user32) fn retire_thread(&mut self, thread: u32) {
        self.entries.retain(|hook| hook.thread != thread);
    }
}
