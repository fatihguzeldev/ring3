pub mod arena;
pub mod call_frame;
pub mod callback;
#[forbid(unsafe_code)]
mod header;
pub mod memory_helper;
pub mod resident_callback;
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
}
