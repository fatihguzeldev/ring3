use crate::loader::{SystemExport32, SystemExportTarget32, SystemModule32};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum WinmmApi32 {
    TimeGetTime = 0x000e_0001,
    TimeBeginPeriod = 0x000e_0002,
    TimeEndPeriod = 0x000e_0003,
    TimeGetDevCaps = 0x000e_0004,
    TimeGetSystemTime = 0x000e_0005,
}
impl WinmmApi32 {
    pub const ALL: [Self; 5] = [
        Self::TimeGetTime,
        Self::TimeBeginPeriod,
        Self::TimeEndPeriod,
        Self::TimeGetDevCaps,
        Self::TimeGetSystemTime,
    ];
    pub fn from_id(id: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|api| *api as u32 == id)
    }
    pub fn id(self) -> u32 {
        self as u32
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::TimeGetTime => "timeGetTime",
            Self::TimeBeginPeriod => "timeBeginPeriod",
            Self::TimeEndPeriod => "timeEndPeriod",
            Self::TimeGetDevCaps => "timeGetDevCaps",
            Self::TimeGetSystemTime => "timeGetSystemTime",
        }
    }
    pub fn stack_words(self) -> u32 {
        match self {
            Self::TimeGetTime => 0,
            Self::TimeBeginPeriod | Self::TimeEndPeriod => 1,
            _ => 2,
        }
    }
}
pub fn winmm_module(preferred_base: u32) -> SystemModule32 {
    SystemModule32 {
        name: "winmm.dll".into(),
        preferred_base,
        exports: WinmmApi32::ALL
            .into_iter()
            .map(|api| SystemExport32 {
                name: Some(api.name().into()),
                ordinal: None,
                target: SystemExportTarget32::Gate(api.id()),
            })
            .collect(),
    }
}
