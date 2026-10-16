mod api;
mod arguments;
mod folders;

pub use api::{ShellApi32, shell32_module};
pub(crate) use arguments::command_line_arguments;
pub use folders::ShellFolderProfile32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellError32 {
    InvalidParameter,
    Allocation,
    Capacity,
    Memory(crate::memory::MemoryError),
    Win32(u32),
}
impl ShellError32 {
    pub fn win32(self) -> u32 {
        match self {
            Self::InvalidParameter => 87,
            Self::Allocation | Self::Capacity => 8,
            Self::Memory(_) => 998,
            Self::Win32(code) => code,
        }
    }
    pub(crate) fn hresult(self) -> u32 {
        if self == Self::InvalidParameter {
            0x8007_0057
        } else {
            0x8007_0000 | self.win32()
        }
    }
}
#[cfg(test)]
mod tests;
