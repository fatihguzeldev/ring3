use crate::{
    loader::{SystemExport32, SystemExportTarget32, SystemModule32},
    windows::CallingConvention32,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Oleaut32Api32 {
    SysAllocString = 0x0020_0001,
    SysAllocStringLen = 0x0020_0002,
    SysAllocStringByteLen = 0x0020_0003,
    SysReAllocString = 0x0020_0004,
    SysReAllocStringLen = 0x0020_0005,
    SysFreeString = 0x0020_0006,
    SysStringLen = 0x0020_0007,
    SysStringByteLen = 0x0020_0008,
    SafeArrayCreate = 0x0020_0009,
    SafeArrayCreateVector = 0x0020_000a,
    SafeArrayDestroy = 0x0020_000b,
    SafeArrayGetDim = 0x0020_000c,
    SafeArrayGetElemsize = 0x0020_000d,
    SafeArrayGetLBound = 0x0020_000e,
    SafeArrayGetUBound = 0x0020_000f,
    SafeArrayLock = 0x0020_0010,
    SafeArrayUnlock = 0x0020_0011,
    SafeArrayAccessData = 0x0020_0012,
    SafeArrayUnaccessData = 0x0020_0013,
    SafeArrayGetElement = 0x0020_0014,
    SafeArrayPutElement = 0x0020_0015,
    SafeArrayCopy = 0x0020_0016,
    SafeArrayRedim = 0x0020_0017,
    SafeArrayGetVartype = 0x0020_0018,
    VariantInit = 0x0020_0019,
    VariantClear = 0x0020_001a,
    VariantCopy = 0x0020_001b,
    VariantCopyInd = 0x0020_001c,
    VariantChangeType = 0x0020_001d,
    VariantChangeTypeEx = 0x0020_001e,
}
impl Oleaut32Api32 {
    pub const ALL: &'static [Self] = &[
        Self::SysAllocString,
        Self::SysAllocStringLen,
        Self::SysAllocStringByteLen,
        Self::SysReAllocString,
        Self::SysReAllocStringLen,
        Self::SysFreeString,
        Self::SysStringLen,
        Self::SysStringByteLen,
        Self::SafeArrayCreate,
        Self::SafeArrayCreateVector,
        Self::SafeArrayDestroy,
        Self::SafeArrayGetDim,
        Self::SafeArrayGetElemsize,
        Self::SafeArrayGetLBound,
        Self::SafeArrayGetUBound,
        Self::SafeArrayLock,
        Self::SafeArrayUnlock,
        Self::SafeArrayAccessData,
        Self::SafeArrayUnaccessData,
        Self::SafeArrayGetElement,
        Self::SafeArrayPutElement,
        Self::SafeArrayCopy,
        Self::SafeArrayRedim,
        Self::SafeArrayGetVartype,
        Self::VariantInit,
        Self::VariantClear,
        Self::VariantCopy,
        Self::VariantCopyInd,
        Self::VariantChangeType,
        Self::VariantChangeTypeEx,
    ];
    pub fn from_id(id: u32) -> Option<Self> {
        Self::ALL
            .get(id.checked_sub(0x0020_0001)? as usize)
            .copied()
    }
    pub fn id(self) -> u32 {
        self as u32
    }
    pub fn convention(self) -> CallingConvention32 {
        CallingConvention32::Stdcall
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::SysAllocString => "SysAllocString",
            Self::SysAllocStringLen => "SysAllocStringLen",
            Self::SysAllocStringByteLen => "SysAllocStringByteLen",
            Self::SysReAllocString => "SysReAllocString",
            Self::SysReAllocStringLen => "SysReAllocStringLen",
            Self::SysFreeString => "SysFreeString",
            Self::SysStringLen => "SysStringLen",
            Self::SysStringByteLen => "SysStringByteLen",
            Self::SafeArrayCreate => "SafeArrayCreate",
            Self::SafeArrayCreateVector => "SafeArrayCreateVector",
            Self::SafeArrayDestroy => "SafeArrayDestroy",
            Self::SafeArrayGetDim => "SafeArrayGetDim",
            Self::SafeArrayGetElemsize => "SafeArrayGetElemsize",
            Self::SafeArrayGetLBound => "SafeArrayGetLBound",
            Self::SafeArrayGetUBound => "SafeArrayGetUBound",
            Self::SafeArrayLock => "SafeArrayLock",
            Self::SafeArrayUnlock => "SafeArrayUnlock",
            Self::SafeArrayAccessData => "SafeArrayAccessData",
            Self::SafeArrayUnaccessData => "SafeArrayUnaccessData",
            Self::SafeArrayGetElement => "SafeArrayGetElement",
            Self::SafeArrayPutElement => "SafeArrayPutElement",
            Self::SafeArrayCopy => "SafeArrayCopy",
            Self::SafeArrayRedim => "SafeArrayRedim",
            Self::SafeArrayGetVartype => "SafeArrayGetVartype",
            Self::VariantInit => "VariantInit",
            Self::VariantClear => "VariantClear",
            Self::VariantCopy => "VariantCopy",
            Self::VariantCopyInd => "VariantCopyInd",
            Self::VariantChangeType => "VariantChangeType",
            Self::VariantChangeTypeEx => "VariantChangeTypeEx",
        }
    }
    pub fn stack_words(self) -> u32 {
        match self {
            Self::SysAllocString => 1,
            Self::SysAllocStringLen => 2,
            Self::SysAllocStringByteLen => 2,
            Self::SysReAllocString => 2,
            Self::SysReAllocStringLen => 3,
            Self::SysFreeString => 1,
            Self::SysStringLen => 1,
            Self::SysStringByteLen => 1,
            Self::SafeArrayCreate => 3,
            Self::SafeArrayCreateVector => 3,
            Self::SafeArrayDestroy => 1,
            Self::SafeArrayGetDim => 1,
            Self::SafeArrayGetElemsize => 1,
            Self::SafeArrayGetLBound => 3,
            Self::SafeArrayGetUBound => 3,
            Self::SafeArrayLock => 1,
            Self::SafeArrayUnlock => 1,
            Self::SafeArrayAccessData => 2,
            Self::SafeArrayUnaccessData => 1,
            Self::SafeArrayGetElement => 3,
            Self::SafeArrayPutElement => 3,
            Self::SafeArrayCopy => 2,
            Self::SafeArrayRedim => 2,
            Self::SafeArrayGetVartype => 2,
            Self::VariantInit | Self::VariantClear => 1,
            Self::VariantCopy | Self::VariantCopyInd => 2,
            Self::VariantChangeType => 4,
            Self::VariantChangeTypeEx => 5,
        }
    }
    pub fn ordinal(self) -> u16 {
        match self {
            Self::SysAllocString => 2,
            Self::SysAllocStringLen => 4,
            Self::SysAllocStringByteLen => 150,
            Self::SysReAllocString => 3,
            Self::SysReAllocStringLen => 5,
            Self::SysFreeString => 6,
            Self::SysStringLen => 7,
            Self::SysStringByteLen => 149,
            Self::SafeArrayCreate => 15,
            Self::SafeArrayCreateVector => 411,
            Self::SafeArrayDestroy => 16,
            Self::SafeArrayGetDim => 17,
            Self::SafeArrayGetElemsize => 18,
            Self::SafeArrayGetLBound => 20,
            Self::SafeArrayGetUBound => 19,
            Self::SafeArrayLock => 21,
            Self::SafeArrayUnlock => 22,
            Self::SafeArrayAccessData => 23,
            Self::SafeArrayUnaccessData => 24,
            Self::SafeArrayGetElement => 25,
            Self::SafeArrayPutElement => 26,
            Self::SafeArrayCopy => 27,
            Self::SafeArrayRedim => 40,
            Self::SafeArrayGetVartype => 77,
            Self::VariantInit => 8,
            Self::VariantClear => 9,
            Self::VariantCopy => 10,
            Self::VariantCopyInd => 11,
            Self::VariantChangeType => 12,
            Self::VariantChangeTypeEx => 147,
        }
    }
    pub(crate) fn is_array(self) -> bool {
        (Self::SafeArrayCreate.id()..=Self::SafeArrayGetVartype.id()).contains(&self.id())
    }
    pub(crate) fn is_variant(self) -> bool {
        self.id() >= Self::VariantInit.id()
    }
    pub(crate) fn returns_hresult(self) -> bool {
        (self.is_array() || self.is_variant())
            && !matches!(
                self,
                Self::SafeArrayCreate
                    | Self::SafeArrayCreateVector
                    | Self::SafeArrayGetDim
                    | Self::SafeArrayGetElemsize
                    | Self::VariantInit
            )
    }
}
pub fn oleaut32_module(base: u32) -> SystemModule32 {
    SystemModule32 {
        name: "oleaut32.dll".into(),
        preferred_base: base,
        exports: Oleaut32Api32::ALL
            .iter()
            .map(|api| SystemExport32 {
                name: Some(api.name().into()),
                ordinal: Some(api.ordinal()),
                target: SystemExportTarget32::Gate(api.id()),
            })
            .collect(),
    }
}
