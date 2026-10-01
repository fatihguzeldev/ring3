mod header;
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
}
