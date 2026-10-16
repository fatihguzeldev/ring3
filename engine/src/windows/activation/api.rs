use crate::loader::{SystemExport32, SystemExportTarget32};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ActivationApi32 {
    CreateActCtxA = 0x0018_0001,
    CreateActCtxW = 0x0018_0002,
    AddRefActCtx = 0x0018_0003,
    ReleaseActCtx = 0x0018_0004,
    ActivateActCtx = 0x0018_0005,
    DeactivateActCtx = 0x0018_0006,
    GetCurrentActCtx = 0x0018_0007,
    FindActCtxSectionStringA = 0x0018_0008,
    FindActCtxSectionStringW = 0x0018_0009,
    FindActCtxSectionGuid = 0x0018_000a,
    QueryActCtxW = 0x0018_000b,
    QueryActCtxSettingsW = 0x0018_000c,
    ZombifyActCtx = 0x0018_000d,
}
impl ActivationApi32 {
    pub const ALL: [Self; 13] = [
        Self::CreateActCtxA,
        Self::CreateActCtxW,
        Self::AddRefActCtx,
        Self::ReleaseActCtx,
        Self::ActivateActCtx,
        Self::DeactivateActCtx,
        Self::GetCurrentActCtx,
        Self::FindActCtxSectionStringA,
        Self::FindActCtxSectionStringW,
        Self::FindActCtxSectionGuid,
        Self::QueryActCtxW,
        Self::QueryActCtxSettingsW,
        Self::ZombifyActCtx,
    ];
    pub fn from_id(id: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|api| api.id() == id)
    }
    pub fn id(self) -> u32 {
        self as u32
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::CreateActCtxA => "CreateActCtxA",
            Self::CreateActCtxW => "CreateActCtxW",
            Self::AddRefActCtx => "AddRefActCtx",
            Self::ReleaseActCtx => "ReleaseActCtx",
            Self::ActivateActCtx => "ActivateActCtx",
            Self::DeactivateActCtx => "DeactivateActCtx",
            Self::GetCurrentActCtx => "GetCurrentActCtx",
            Self::FindActCtxSectionStringA => "FindActCtxSectionStringA",
            Self::FindActCtxSectionStringW => "FindActCtxSectionStringW",
            Self::FindActCtxSectionGuid => "FindActCtxSectionGuid",
            Self::QueryActCtxW => "QueryActCtxW",
            Self::QueryActCtxSettingsW => "QueryActCtxSettingsW",
            Self::ZombifyActCtx => "ZombifyActCtx",
        }
    }
    pub fn stack_words(self) -> u32 {
        match self {
            Self::ActivateActCtx | Self::DeactivateActCtx => 2,
            Self::FindActCtxSectionStringA
            | Self::FindActCtxSectionStringW
            | Self::FindActCtxSectionGuid => 5,
            Self::QueryActCtxW | Self::QueryActCtxSettingsW => 7,
            _ => 1,
        }
    }
    pub fn failure_value(self) -> u32 {
        if matches!(self, Self::CreateActCtxA | Self::CreateActCtxW) {
            u32::MAX
        } else {
            0
        }
    }
}
pub fn kernel32_activation_exports() -> Vec<SystemExport32> {
    ActivationApi32::ALL
        .into_iter()
        .map(|api| SystemExport32 {
            name: Some(api.name().into()),
            ordinal: None,
            target: SystemExportTarget32::Gate(api.id()),
        })
        .collect()
}
