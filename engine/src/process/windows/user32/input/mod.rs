mod context;
mod delivery;
mod focus;
mod pointer;
mod queries;
mod runtime;
mod translation;

pub(super) use context::User32Input32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct User32CursorRequest32 {
    pub revision: u64,
    pub changes: u32,
    pub position: crate::windows::user32::Point32,
    pub clip: Option<crate::windows::user32::Rect32>,
    pub visible: bool,
    pub cursor: u32,
}
