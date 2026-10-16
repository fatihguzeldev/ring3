#![forbid(unsafe_code)]

pub(super) mod calls;
pub(super) mod virtual_memory;

pub(super) mod activation;
pub(super) mod callbacks;
pub(super) mod d3d8;
pub(super) mod dinput;
pub(super) mod dsound;
pub(super) mod files;
pub(super) mod modules;
pub(super) mod objects;
pub(super) mod service;
mod strings;
pub(super) mod synchronization;
mod thread;
pub(super) mod user32;

pub(super) mod ole32;
pub(super) mod winmm;

pub(super) mod process;

pub(super) mod memory;

pub(super) mod threads;

pub(super) mod registry;

pub(super) mod exceptions;
pub(super) mod nls;

pub(super) mod resources;

pub(super) mod console;
pub(super) mod native;
pub(super) mod security;

pub(super) mod version;

pub(super) mod shlwapi;
pub(super) mod shell;
