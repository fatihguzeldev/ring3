use crate::{
    loader::{SystemExport32, SystemExportTarget32, SystemModule32},
    windows::CallingConvention32,
};

pub(crate) const COMMON_PROC_A: u32 = 0x0022_ff01;
pub(crate) const COMMON_PROC_W: u32 = 0x0022_ff02;
pub(crate) const TOOLTIP_RELAY_PROC: u32 = 0x0022_ff03;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum CommonControlsApi32 {
    InitCommonControlsEx = 0x0022_0002,
    CreateStatusWindowA = 0x0022_0003,
    CreateStatusWindowW = 0x0022_0004,
}

impl CommonControlsApi32 {
    pub const ALL: &'static [Self] = &[
        Self::InitCommonControlsEx,
        Self::CreateStatusWindowA,
        Self::CreateStatusWindowW,
    ];

    pub fn from_id(id: u32) -> Option<Self> {
        Self::ALL.iter().copied().find(|api| api.id() == id)
    }

    pub const fn id(self) -> u32 {
        self as u32
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::InitCommonControlsEx => "InitCommonControlsEx",
            Self::CreateStatusWindowA => "CreateStatusWindowA",
            Self::CreateStatusWindowW => "CreateStatusWindowW",
        }
    }

    pub const fn stack_words(self) -> u32 {
        match self {
            Self::InitCommonControlsEx => 1,
            Self::CreateStatusWindowA | Self::CreateStatusWindowW => 4,
        }
    }

    pub const fn convention(self) -> CallingConvention32 {
        CallingConvention32::Stdcall
    }
}

pub fn comctl32_module(preferred_base: u32) -> SystemModule32 {
    let mut exports: Vec<SystemExport32> = CommonControlsApi32::ALL
        .iter()
        .map(|api| SystemExport32 {
            name: Some(api.name().into()),
            ordinal: None,
            target: SystemExportTarget32::Gate(api.id()),
        })
        .collect();
    exports.extend(
        [COMMON_PROC_A, COMMON_PROC_W, TOOLTIP_RELAY_PROC]
            .into_iter()
            .map(|id| SystemExport32 {
                name: None,
                ordinal: None,
                target: SystemExportTarget32::PrivateGate(id),
            }),
    );
    SystemModule32 {
        name: "comctl32.dll".into(),
        preferred_base,
        exports,
    }
}
