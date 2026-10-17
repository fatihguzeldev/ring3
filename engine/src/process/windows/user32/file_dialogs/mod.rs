mod callbacks;
mod controls;
mod messages;
mod multiple;
mod paths;
mod procedure;
mod prompts;
mod requests;
mod template;

use crate::{
    process::windows::commdlg::CapturedFileDialog32,
    windows::{user32::User32Error32 as E, window::WindowHandle32},
};
pub(super) use callbacks::FileDialogCall32;
pub(super) use prompts::FilePrompt32;

pub(super) struct FileEntry32 {
    path: Vec<u16>,
    name: Vec<u16>,
    directory: bool,
}

pub(super) struct Selection32 {
    path: Vec<u16>,
    readonly: bool,
    additional: Vec<Vec<u16>>,
}

pub(super) struct FileDialog32 {
    token: u64,
    call_owner: crate::process::calls::call::PendingOwner,
    call_token: u32,
    original_directory: Vec<u16>,
    completed: bool,
    thread: u32,
    root: Option<WindowHandle32>,
    request: CapturedFileDialog32,
    save: bool,
    initialized: bool,
    published_error: Option<u32>,
    selection: Option<Selection32>,
    directory: Vec<u16>,
    entries: Vec<FileEntry32>,
    filter_index: u32,
}

pub(super) struct FileDialogs32 {
    plans: Vec<FileDialog32>,
    limit: usize,
}
impl FileDialogs32 {
    pub(super) fn new(limit: usize) -> Result<Self, E> {
        let mut plans = Vec::new();
        plans.try_reserve_exact(limit).map_err(|_| E::Allocation)?;
        Ok(Self { plans, limit })
    }
    pub(super) fn retire_thread(&mut self, thread: u32) {
        self.plans.retain(|plan| plan.thread != thread);
    }
    pub(super) fn retire(&mut self, token: u64) {
        self.plans.retain(|plan| plan.token != token);
    }
    pub(super) fn complete(&mut self, token: u64, result: u32) -> u32 {
        if let Some(plan) = self.plans.iter_mut().find(|plan| plan.token == token) {
            plan.completed = true;
            if result > 1 {
                plan.published_error = Some(0xffff);
                return 0;
            }
        }
        result
    }
    fn index(&self, token: u64) -> Result<usize, E> {
        self.plans
            .iter()
            .position(|plan| plan.token == token)
            .ok_or(E::InvalidToken)
    }
    fn window(&self, root: WindowHandle32) -> Result<usize, E> {
        self.plans
            .iter()
            .position(|plan| plan.root == Some(root))
            .ok_or(E::InvalidToken)
    }
}

#[cfg(test)]
mod tests;
