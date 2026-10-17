use crate::loader::{SystemExport32, SystemExportTarget32, SystemModule32};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ImmApi32 {
    ImmCreateContext,
    ImmDestroyContext,
    ImmAssociateContext,
    ImmGetContext,
    ImmReleaseContext,
    ImmGetOpenStatus,
    ImmSetOpenStatus,
    ImmGetConversionStatus,
    ImmSetConversionStatus,
    ImmGetCompositionWindow,
    ImmSetCompositionWindow,
    ImmGetCandidateWindow,
    ImmSetCandidateWindow,
    ImmGetCompositionFontA,
    ImmGetCompositionFontW,
    ImmSetCompositionFontA,
    ImmSetCompositionFontW,
    ImmGetCompositionStringA,
    ImmGetCompositionStringW,
}
impl ImmApi32 {
    pub const ALL: &'static [Self] = &[
        Self::ImmCreateContext,
        Self::ImmDestroyContext,
        Self::ImmAssociateContext,
        Self::ImmGetContext,
        Self::ImmReleaseContext,
        Self::ImmGetOpenStatus,
        Self::ImmSetOpenStatus,
        Self::ImmGetConversionStatus,
        Self::ImmSetConversionStatus,
        Self::ImmGetCompositionWindow,
        Self::ImmSetCompositionWindow,
        Self::ImmGetCandidateWindow,
        Self::ImmSetCandidateWindow,
        Self::ImmGetCompositionFontA,
        Self::ImmGetCompositionFontW,
        Self::ImmSetCompositionFontA,
        Self::ImmSetCompositionFontW,
        Self::ImmGetCompositionStringA,
        Self::ImmGetCompositionStringW,
    ];
    pub fn from_id(id: u32) -> Option<Self> {
        Self::ALL
            .get(id.checked_sub(0x0023_0001)? as usize)
            .copied()
    }
    pub fn id(self) -> u32 {
        0x0023_0001 + self as u32
    }
    pub fn stack_words(self) -> u32 {
        match self {
            Self::ImmGetCandidateWindow => 3,
            Self::ImmGetCompositionStringA | Self::ImmGetCompositionStringW => 4,
            Self::ImmGetCompositionWindow
            | Self::ImmSetCompositionWindow
            | Self::ImmSetCandidateWindow
            | Self::ImmGetCompositionFontA
            | Self::ImmGetCompositionFontW
            | Self::ImmSetCompositionFontA
            | Self::ImmSetCompositionFontW => 2,
            Self::ImmCreateContext => 0,
            Self::ImmDestroyContext | Self::ImmGetContext | Self::ImmGetOpenStatus => 1,
            Self::ImmAssociateContext | Self::ImmReleaseContext | Self::ImmSetOpenStatus => 2,
            Self::ImmGetConversionStatus | Self::ImmSetConversionStatus => 3,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::ImmGetCompositionWindow => "ImmGetCompositionWindow",
            Self::ImmSetCompositionWindow => "ImmSetCompositionWindow",
            Self::ImmGetCandidateWindow => "ImmGetCandidateWindow",
            Self::ImmSetCandidateWindow => "ImmSetCandidateWindow",
            Self::ImmGetCompositionFontA => "ImmGetCompositionFontA",
            Self::ImmGetCompositionFontW => "ImmGetCompositionFontW",
            Self::ImmSetCompositionFontA => "ImmSetCompositionFontA",
            Self::ImmSetCompositionFontW => "ImmSetCompositionFontW",
            Self::ImmGetCompositionStringA => "ImmGetCompositionStringA",
            Self::ImmGetCompositionStringW => "ImmGetCompositionStringW",
            Self::ImmCreateContext => "ImmCreateContext",
            Self::ImmDestroyContext => "ImmDestroyContext",
            Self::ImmAssociateContext => "ImmAssociateContext",
            Self::ImmGetContext => "ImmGetContext",
            Self::ImmReleaseContext => "ImmReleaseContext",
            Self::ImmGetOpenStatus => "ImmGetOpenStatus",
            Self::ImmSetOpenStatus => "ImmSetOpenStatus",
            Self::ImmGetConversionStatus => "ImmGetConversionStatus",
            Self::ImmSetConversionStatus => "ImmSetConversionStatus",
        }
    }
}
pub fn imm32_module(preferred_base: u32) -> SystemModule32 {
    SystemModule32 {
        name: "imm32.dll".into(),
        preferred_base,
        exports: ImmApi32::ALL
            .iter()
            .map(|api| SystemExport32 {
                name: Some(api.name().into()),
                ordinal: None,
                target: SystemExportTarget32::Gate(api.id()),
            })
            .collect(),
    }
}
