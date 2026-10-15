#![forbid(unsafe_code)]

pub(super) mod calls;
pub(super) mod virtual_memory;

pub(super) mod callbacks;
pub(super) mod d3d8;
pub(super) mod dinput;
pub(super) mod dsound;
pub(super) mod modules;
pub(super) mod objects;
mod strings;
pub(super) mod synchronization;
mod thread;
pub(super) mod user32;
