use super::preparation::Prepared32;
use crate::{
    memory::GuestAddress, process::windows::user32::User32CallContext32,
    windows::window::WindowHandle32,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Stage32 {
    Create(usize),
    Font(usize),
    Disable,
    Initialize,
    Focus,
    Show,
    Wait,
    Retrieve,
    Dispatch,
    Destroy,
    Restore,
    RestoreFocus,
}

pub(super) struct Plan32 {
    pub token: u64,
    pub thread: u32,
    pub context: User32CallContext32,
    pub prepared: Prepared32,
    pub windows: Vec<WindowHandle32>,
    pub procedure: GuestAddress,
    pub parameter: u32,
    pub owner: Option<WindowHandle32>,
    pub owner_enabled: bool,
    pub owner_disabled: bool,
    pub previous_focus: Option<WindowHandle32>,
    pub modal: bool,
    pub wide: bool,
    pub font: u32,
    pub stage: Stage32,
    pub result: u32,
}

pub(super) struct FontLease32 {
    pub window: Option<WindowHandle32>,
    pub font: u32,
}

pub(in crate::process::windows::user32) struct ResourceDialogs32 {
    pub(in crate::process::windows::user32) procedures: Option<[GuestAddress; 2]>,
    pub(in crate::process::windows::user32) defaults: Option<[GuestAddress; 2]>,
    pub(in crate::process::windows::user32) system_base: crate::windows::user32::Point32,
    pub(super) plans: Vec<Plan32>,
    pub(super) fonts: Vec<FontLease32>,
    pub(super) limit: usize,
}

impl ResourceDialogs32 {
    pub(in crate::process::windows::user32) fn new(
        continuations: usize,
        windows: usize,
    ) -> Result<Self, crate::windows::user32::User32Error32> {
        let mut plans = Vec::new();
        plans
            .try_reserve_exact(continuations)
            .map_err(|_| crate::windows::user32::User32Error32::Allocation)?;
        let mut fonts = Vec::new();
        fonts
            .try_reserve_exact(windows)
            .map_err(|_| crate::windows::user32::User32Error32::Allocation)?;
        Ok(Self {
            procedures: None,
            defaults: None,
            system_base: Default::default(),
            plans,
            fonts,
            limit: continuations,
        })
    }
}
