mod callbacks;
mod devices;
mod objects;
mod semantics;
use crate::{
    input::dinput::InterfaceKind32,
    memory::{GuestAddress, PageRange},
};
pub(super) struct PendingEnumeration32 {
    pub token: u64,
    pub procedure: GuestAddress,
    pub context: u32,
    pub interface: InterfaceKind32,
    pub rows: Vec<Vec<u8>>,
    pub cursor: usize,
    pub scratch: GuestAddress,
    pub range: PageRange,
    pub mapping: u64,
    pub memory: u64,
    pub semantics: Option<semantics::SemanticEnumeration32>,
    pub resuming: bool,
    pub next_row: Option<usize>,
}

pub(super) fn device_index(kind: crate::input::dinput::DeviceKind32) -> usize {
    match kind {
        crate::input::dinput::DeviceKind32::Keyboard => 0,
        crate::input::dinput::DeviceKind32::Mouse => 1,
        crate::input::dinput::DeviceKind32::Controller(slot) => usize::from(slot) + 2,
    }
}
