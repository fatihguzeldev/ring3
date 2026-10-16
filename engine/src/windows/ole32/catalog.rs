use crate::loader::{SystemExport32, SystemExportTarget32, SystemModule32};
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ole32Api32 {
    CoInitialize = 0x000d_0001,
    CoInitializeEx = 0x000d_0002,
    CoUninitialize = 0x000d_0003,
    CoCreateInstance = 0x000d_0004,
    CoGetClassObject = 0x000d_0005,
    CoFreeUnusedLibraries = 0x000d_0006,
    CoFreeUnusedLibrariesEx = 0x000d_0007,
    CoTaskMemAlloc = 0x000d_0008,
    CoTaskMemRealloc = 0x000d_0009,
    CoTaskMemFree = 0x000d_000a,
}
impl Ole32Api32 {
    pub const ALL: [Self; 10] = [
        Self::CoInitialize,
        Self::CoInitializeEx,
        Self::CoUninitialize,
        Self::CoCreateInstance,
        Self::CoGetClassObject,
        Self::CoFreeUnusedLibraries,
        Self::CoFreeUnusedLibrariesEx,
        Self::CoTaskMemAlloc,
        Self::CoTaskMemRealloc,
        Self::CoTaskMemFree,
    ];
    pub fn from_id(id: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|api| *api as u32 == id)
    }
    pub fn id(self) -> u32 {
        self as u32
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::CoInitialize => "CoInitialize",
            Self::CoInitializeEx => "CoInitializeEx",
            Self::CoUninitialize => "CoUninitialize",
            Self::CoCreateInstance => "CoCreateInstance",
            Self::CoGetClassObject => "CoGetClassObject",
            Self::CoFreeUnusedLibraries => "CoFreeUnusedLibraries",
            Self::CoFreeUnusedLibrariesEx => "CoFreeUnusedLibrariesEx",
            Self::CoTaskMemAlloc => "CoTaskMemAlloc",
            Self::CoTaskMemRealloc => "CoTaskMemRealloc",
            Self::CoTaskMemFree => "CoTaskMemFree",
        }
    }
    pub fn stack_words(self) -> u32 {
        match self {
            Self::CoInitialize | Self::CoTaskMemAlloc | Self::CoTaskMemFree => 1,
            Self::CoInitializeEx | Self::CoFreeUnusedLibrariesEx | Self::CoTaskMemRealloc => 2,
            Self::CoCreateInstance | Self::CoGetClassObject => 5,
            _ => 0,
        }
    }
    pub(crate) fn is_task_memory(self) -> bool {
        matches!(
            self,
            Self::CoTaskMemAlloc | Self::CoTaskMemRealloc | Self::CoTaskMemFree
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FactoryMethod32(pub u8);
impl FactoryMethod32 {
    pub fn from_id(id: u32) -> Option<Self> {
        (0x000d_1000..0x000d_1005)
            .contains(&id)
            .then(|| Self((id - 0x000d_1000) as u8))
    }
    pub fn id(self) -> u32 {
        0x000d_1000 + u32::from(self.0)
    }
    pub fn stack_words(self) -> u32 {
        match self.0 {
            0 => 3,
            1 | 2 => 1,
            3 => 4,
            4 => 2,
            _ => 0,
        }
    }
}
pub fn ole32_module(preferred_base: u32) -> SystemModule32 {
    let mut exports: Vec<_> = Ole32Api32::ALL
        .into_iter()
        .map(|api| SystemExport32 {
            name: Some(api.name().into()),
            ordinal: None,
            target: SystemExportTarget32::Gate(api.id()),
        })
        .collect();
    exports.extend((0..5).map(|slot| SystemExport32 {
        name: None,
        ordinal: None,
        target: SystemExportTarget32::PrivateGate(FactoryMethod32(slot).id()),
    }));
    SystemModule32 {
        name: "ole32.dll".into(),
        preferred_base,
        exports,
    }
}
