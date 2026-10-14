pub mod arena;
pub mod call_frame;
pub mod callback;
pub mod cpu_helper;
pub mod exception;
pub mod far;
#[forbid(unsafe_code)]
pub(crate) mod header;
pub mod memory_helper;
pub mod resident_callback;
pub mod tasks;
#[cfg(target_arch = "wasm32")]
mod wasm;
#[forbid(unsafe_code)]
pub mod x86;

pub const ABI_VERSION: u16 = 1;
pub const X86_INTEGER_PROFILE: u16 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbiError {
    Length,
    Magic,
    Version,
    Profile,
    Reserved,
    Flags,
    Exit,
    MemoryHelper,
    CallFrame,
    CallbackFrame,
    TaskSources,
    TaskContext,
    TaskControls,
}

pub mod interrupt;
