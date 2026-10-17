use crate::{
    loader::{SystemExport32, SystemExportTarget32, SystemModule32},
    windows::CallingConvention32,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ShellApi32 {
    SHGetFolderPathA = 0x001d_0001,
    SHGetFolderPathW = 0x001d_0002,
    SHGetSpecialFolderPathA = 0x001d_0003,
    SHGetSpecialFolderPathW = 0x001d_0004,
    CommandLineToArgvW = 0x001d_0005,
}
impl ShellApi32 {
    pub const ALL: &'static [Self] = &[
        Self::SHGetFolderPathA,
        Self::SHGetFolderPathW,
        Self::SHGetSpecialFolderPathA,
        Self::SHGetSpecialFolderPathW,
        Self::CommandLineToArgvW,
    ];
    pub fn from_id(id: u32) -> Option<Self> {
        Self::ALL
            .get(id.checked_sub(0x001d_0001)? as usize)
            .copied()
    }
    pub fn id(self) -> u32 {
        self as u32
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::SHGetFolderPathA => "SHGetFolderPathA",
            Self::SHGetFolderPathW => "SHGetFolderPathW",
            Self::SHGetSpecialFolderPathA => "SHGetSpecialFolderPathA",
            Self::SHGetSpecialFolderPathW => "SHGetSpecialFolderPathW",
            Self::CommandLineToArgvW => "CommandLineToArgvW",
        }
    }
    pub fn stack_words(self) -> u32 {
        match self {
            Self::SHGetFolderPathA => 5,
            Self::SHGetFolderPathW => 5,
            Self::SHGetSpecialFolderPathA => 4,
            Self::SHGetSpecialFolderPathW => 4,
            Self::CommandLineToArgvW => 2,
        }
    }
    pub fn convention(self) -> CallingConvention32 {
        CallingConvention32::Stdcall
    }
}
pub fn shell32_module(preferred_base: u32) -> SystemModule32 {
    SystemModule32 {
        name: "shell32.dll".into(),
        preferred_base,
        exports: ShellApi32::ALL
            .iter()
            .map(|api| SystemExport32 {
                name: Some(api.name().into()),
                ordinal: None,
                target: SystemExportTarget32::Gate(api.id()),
            })
            .collect(),
    }
}

pub fn shfolder_module(preferred_base: u32) -> SystemModule32 {
    SystemModule32 {
        name: "shfolder.dll".into(),
        preferred_base,
        exports: [ShellApi32::SHGetFolderPathA, ShellApi32::SHGetFolderPathW]
            .into_iter()
            .map(|api| SystemExport32 {
                name: Some(api.name().into()),
                ordinal: None,
                target: SystemExportTarget32::Forwarder(format!("shell32.{}", api.name())),
            })
            .collect(),
    }
}
