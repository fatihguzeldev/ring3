use crate::{
    loader::{SystemExport32, SystemExportTarget32, SystemModule32},
    windows::CallingConvention32,
};

pub(crate) const DIALOG_PROCEDURE_A: u32 = 0x0021_ff01;
pub(crate) const DIALOG_PROCEDURE_W: u32 = 0x0021_ff02;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ComdlgApi32 {
    GetOpenFileNameA = 0x0021_0001,
    GetOpenFileNameW = 0x0021_0002,
    GetSaveFileNameA = 0x0021_0003,
    GetSaveFileNameW = 0x0021_0004,
    CommDlgExtendedError = 0x0021_0005,
    GetFileTitleA = 0x0021_0006,
    GetFileTitleW = 0x0021_0007,
}

impl ComdlgApi32 {
    pub const ALL: &'static [Self] = &[
        Self::GetOpenFileNameA,
        Self::GetOpenFileNameW,
        Self::GetSaveFileNameA,
        Self::GetSaveFileNameW,
        Self::CommDlgExtendedError,
        Self::GetFileTitleA,
        Self::GetFileTitleW,
    ];

    pub fn from_id(id: u32) -> Option<Self> {
        Self::ALL
            .get(id.checked_sub(0x0021_0001)? as usize)
            .copied()
    }

    pub fn id(self) -> u32 {
        self as u32
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::GetOpenFileNameA => "GetOpenFileNameA",
            Self::GetOpenFileNameW => "GetOpenFileNameW",
            Self::GetSaveFileNameA => "GetSaveFileNameA",
            Self::GetSaveFileNameW => "GetSaveFileNameW",
            Self::CommDlgExtendedError => "CommDlgExtendedError",
            Self::GetFileTitleA => "GetFileTitleA",
            Self::GetFileTitleW => "GetFileTitleW",
        }
    }

    pub fn stack_words(self) -> u32 {
        match self {
            Self::CommDlgExtendedError => 0,
            Self::GetFileTitleA | Self::GetFileTitleW => 3,
            _ => 1,
        }
    }

    pub fn convention(self) -> CallingConvention32 {
        CallingConvention32::Stdcall
    }

    pub(crate) fn wide(self) -> bool {
        matches!(
            self,
            Self::GetOpenFileNameW | Self::GetSaveFileNameW | Self::GetFileTitleW
        )
    }

    pub(crate) fn save(self) -> bool {
        matches!(self, Self::GetSaveFileNameA | Self::GetSaveFileNameW)
    }
}

pub fn commdlg32_module(preferred_base: u32) -> SystemModule32 {
    let mut exports = ComdlgApi32::ALL
        .iter()
        .map(|api| SystemExport32 {
            name: Some(api.name().into()),
            ordinal: None,
            target: SystemExportTarget32::Gate(api.id()),
        })
        .collect::<Vec<_>>();
    exports.extend(
        [DIALOG_PROCEDURE_A, DIALOG_PROCEDURE_W].map(|id| SystemExport32 {
            name: None,
            ordinal: None,
            target: SystemExportTarget32::PrivateGate(id),
        }),
    );
    SystemModule32 {
        name: "comdlg32.dll".into(),
        preferred_base,
        exports,
    }
}
