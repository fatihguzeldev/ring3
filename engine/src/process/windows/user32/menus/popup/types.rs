use super::super::visual::MenuVisual32;
use crate::{
    process::windows::user32::User32CallContext32,
    windows::{
        user32::{
            Point32, Rect32, User32Error32 as E,
            menus::{MenuHandle32, MenuTracking32},
        },
        window::WindowHandle32,
    },
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Stage32 {
    Enter,
    Initialize,
    InitializePopup(usize),
    Layout(usize),
    Create(usize),
    Position(usize),
    Show(usize),
    Capture,
    Paint(usize),
    Wait,
    Retrieve,
    Dispatch,
    Select,
    Character,
    Uninitialize(usize),
    Destroy(usize),
    Release,
    FinalSelect,
    Exit,
    Finish,
}

#[derive(Clone, Copy)]
pub(super) enum After32 {
    Wait,
    Open(usize),
    Close(usize),
    Finish,
}

pub(super) struct Window32 {
    pub handle: Option<WindowHandle32>,
    pub menu: MenuHandle32,
    pub position: usize,
    pub origin: Point32,
    pub visual: Option<MenuVisual32>,
}

pub(super) struct Plan32 {
    pub token: u64,
    pub call: User32CallContext32,
    pub owner: WindowHandle32,
    pub root: MenuHandle32,
    pub flags: u32,
    pub anchor: Point32,
    pub exclude: Option<Rect32>,
    pub tracker: MenuTracking32,
    pub windows: Vec<Window32>,
    pub stage: Stage32,
    pub after: After32,
    pub character: Option<u16>,
    pub selected: Option<(usize, Option<usize>)>,
    pub command: Option<(MenuHandle32, usize, u32)>,
    pub cancelled: bool,
    pub captured: bool,
    pub restore_capture: Option<WindowHandle32>,
    pub child: Option<u64>,
    pub completed: Option<u64>,
}

pub(in crate::process::windows::user32) struct PopupContext32 {
    pub(in crate::process::windows::user32) procedure: Option<crate::memory::GuestAddress>,
    pub(super) plans: Vec<Plan32>,
    pub(super) limit: usize,
}

impl PopupContext32 {
    pub(in crate::process::windows::user32) fn new(limit: usize) -> Result<Self, E> {
        let mut plans = Vec::new();
        plans.try_reserve_exact(limit).map_err(|_| E::Allocation)?;
        Ok(Self {
            procedure: None,
            plans,
            limit,
        })
    }
}
