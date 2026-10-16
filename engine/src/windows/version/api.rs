use crate::{
    loader::{SystemExport32, SystemExportTarget32, SystemModule32},
    windows::CallingConvention32,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum VersionApi32 {
    GetFileVersionInfoSizeA = 0x001b_0001,
    GetFileVersionInfoSizeW = 0x001b_0002,
    GetFileVersionInfoA = 0x001b_0003,
    GetFileVersionInfoW = 0x001b_0004,
    VerQueryValueA = 0x001b_0005,
    VerQueryValueW = 0x001b_0006,
}
impl VersionApi32 {
    pub const ALL: &'static [Self] = &[
        Self::GetFileVersionInfoSizeA,
        Self::GetFileVersionInfoSizeW,
        Self::GetFileVersionInfoA,
        Self::GetFileVersionInfoW,
        Self::VerQueryValueA,
        Self::VerQueryValueW,
    ];
    pub fn from_id(id: u32) -> Option<Self> {
        Self::ALL
            .get(id.checked_sub(0x001b_0001)? as usize)
            .copied()
    }
    pub fn id(self) -> u32 {
        self as u32
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::GetFileVersionInfoSizeA => "GetFileVersionInfoSizeA",
            Self::GetFileVersionInfoSizeW => "GetFileVersionInfoSizeW",
            Self::GetFileVersionInfoA => "GetFileVersionInfoA",
            Self::GetFileVersionInfoW => "GetFileVersionInfoW",
            Self::VerQueryValueA => "VerQueryValueA",
            Self::VerQueryValueW => "VerQueryValueW",
        }
    }
    pub fn stack_words(self) -> u32 {
        match self {
            Self::GetFileVersionInfoSizeA => 2,
            Self::GetFileVersionInfoSizeW => 2,
            Self::GetFileVersionInfoA => 4,
            Self::GetFileVersionInfoW => 4,
            Self::VerQueryValueA => 4,
            Self::VerQueryValueW => 4,
        }
    }
    pub fn convention(self) -> CallingConvention32 {
        CallingConvention32::Stdcall
    }
}
pub fn version_module(preferred_base: u32) -> SystemModule32 {
    SystemModule32 {
        name: "version.dll".into(),
        preferred_base,
        exports: VersionApi32::ALL
            .iter()
            .map(|api| SystemExport32 {
                name: Some(api.name().into()),
                ordinal: None,
                target: SystemExportTarget32::Gate(api.id()),
            })
            .collect(),
    }
}
