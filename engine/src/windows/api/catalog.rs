use crate::windows::CallingConvention32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum WindowsApi32 {
    GetLastError = 0x0001_0001,
    SetLastError = 0x0001_0002,
    ExitProcess = 0x0001_0003,
    GetModuleHandleA = 0x0001_0004,
    VirtualAlloc = 0x0001_0005,
    VirtualFree = 0x0001_0006,
    VirtualProtect = 0x0001_0007,
    FlushInstructionCache = 0x0001_0008,
    GetCurrentProcess = 0x0001_0009,
    LoadLibraryA = 0x0001_000a,
    LoadLibraryW = 0x0001_000b,
    LoadLibraryExA = 0x0001_000c,
    LoadLibraryExW = 0x0001_000d,
    GetProcAddress = 0x0001_000e,
    FreeLibrary = 0x0001_000f,
    GetModuleHandleW = 0x0001_0010,
    CreateEventA = 0x0001_0011,
    CreateEventW = 0x0001_0012,
    OpenEventA = 0x0001_0013,
    OpenEventW = 0x0001_0014,
    SetEvent = 0x0001_0015,
    ResetEvent = 0x0001_0016,
    PulseEvent = 0x0001_0017,
    CloseHandle = 0x0001_0018,
    DuplicateHandle = 0x0001_0019,
    WaitForSingleObject = 0x0001_001a,
    WaitForSingleObjectEx = 0x0001_001b,
    WaitForMultipleObjects = 0x0001_001c,
    WaitForMultipleObjectsEx = 0x0001_001d,
    Sleep = 0x0001_001e,
    SleepEx = 0x0001_001f,
    SignalObjectAndWait = 0x0001_0020,
    SetDllDirectoryA = 0x0001_0021,
    SetDllDirectoryW = 0x0001_0022,
    GetDllDirectoryA = 0x0001_0023,
    GetDllDirectoryW = 0x0001_0024,
}

#[derive(Clone, Copy)]
#[repr(u8)]
pub(crate) enum ProviderProfile {
    LastErrorOnly = 1,
    LastErrorAndExit = 2,
    MainImageHandle = 3,
    VirtualAllocation = 4,
    VirtualMemory = 5,
    VirtualProtection = 6,
    InstructionCache = 7,
    CurrentProcess = 8,
    Modules = 9,
    Synchronization = 10,
}

struct ApiDefinition {
    api: WindowsApi32,
    name: &'static str,
    stack_words: u32,
    introduced: ProviderProfile,
}

const APIS: [ApiDefinition; 36] = [
    ApiDefinition {
        api: WindowsApi32::GetLastError,
        name: "GetLastError",
        stack_words: 0,
        introduced: ProviderProfile::LastErrorOnly,
    },
    ApiDefinition {
        api: WindowsApi32::SetLastError,
        name: "SetLastError",
        stack_words: 1,
        introduced: ProviderProfile::LastErrorOnly,
    },
    ApiDefinition {
        api: WindowsApi32::ExitProcess,
        name: "ExitProcess",
        stack_words: 1,
        introduced: ProviderProfile::LastErrorAndExit,
    },
    ApiDefinition {
        api: WindowsApi32::GetModuleHandleA,
        name: "GetModuleHandleA",
        stack_words: 1,
        introduced: ProviderProfile::MainImageHandle,
    },
    ApiDefinition {
        api: WindowsApi32::VirtualAlloc,
        name: "VirtualAlloc",
        stack_words: 4,
        introduced: ProviderProfile::VirtualAllocation,
    },
    ApiDefinition {
        api: WindowsApi32::VirtualFree,
        name: "VirtualFree",
        stack_words: 3,
        introduced: ProviderProfile::VirtualMemory,
    },
    ApiDefinition {
        api: WindowsApi32::VirtualProtect,
        name: "VirtualProtect",
        stack_words: 4,
        introduced: ProviderProfile::VirtualProtection,
    },
    ApiDefinition {
        api: WindowsApi32::FlushInstructionCache,
        name: "FlushInstructionCache",
        stack_words: 3,
        introduced: ProviderProfile::InstructionCache,
    },
    ApiDefinition {
        api: WindowsApi32::GetCurrentProcess,
        name: "GetCurrentProcess",
        stack_words: 0,
        introduced: ProviderProfile::CurrentProcess,
    },
    ApiDefinition {
        api: WindowsApi32::LoadLibraryA,
        name: "LoadLibraryA",
        stack_words: 1,
        introduced: ProviderProfile::Modules,
    },
    ApiDefinition {
        api: WindowsApi32::LoadLibraryW,
        name: "LoadLibraryW",
        stack_words: 1,
        introduced: ProviderProfile::Modules,
    },
    ApiDefinition {
        api: WindowsApi32::LoadLibraryExA,
        name: "LoadLibraryExA",
        stack_words: 3,
        introduced: ProviderProfile::Modules,
    },
    ApiDefinition {
        api: WindowsApi32::LoadLibraryExW,
        name: "LoadLibraryExW",
        stack_words: 3,
        introduced: ProviderProfile::Modules,
    },
    ApiDefinition {
        api: WindowsApi32::GetProcAddress,
        name: "GetProcAddress",
        stack_words: 2,
        introduced: ProviderProfile::Modules,
    },
    ApiDefinition {
        api: WindowsApi32::FreeLibrary,
        name: "FreeLibrary",
        stack_words: 1,
        introduced: ProviderProfile::Modules,
    },
    ApiDefinition {
        api: WindowsApi32::GetModuleHandleW,
        name: "GetModuleHandleW",
        stack_words: 1,
        introduced: ProviderProfile::Modules,
    },
    ApiDefinition {
        api: WindowsApi32::CreateEventA,
        name: "CreateEventA",
        stack_words: 4,
        introduced: ProviderProfile::Synchronization,
    },
    ApiDefinition {
        api: WindowsApi32::CreateEventW,
        name: "CreateEventW",
        stack_words: 4,
        introduced: ProviderProfile::Synchronization,
    },
    ApiDefinition {
        api: WindowsApi32::OpenEventA,
        name: "OpenEventA",
        stack_words: 3,
        introduced: ProviderProfile::Synchronization,
    },
    ApiDefinition {
        api: WindowsApi32::OpenEventW,
        name: "OpenEventW",
        stack_words: 3,
        introduced: ProviderProfile::Synchronization,
    },
    ApiDefinition {
        api: WindowsApi32::SetEvent,
        name: "SetEvent",
        stack_words: 1,
        introduced: ProviderProfile::Synchronization,
    },
    ApiDefinition {
        api: WindowsApi32::ResetEvent,
        name: "ResetEvent",
        stack_words: 1,
        introduced: ProviderProfile::Synchronization,
    },
    ApiDefinition {
        api: WindowsApi32::PulseEvent,
        name: "PulseEvent",
        stack_words: 1,
        introduced: ProviderProfile::Synchronization,
    },
    ApiDefinition {
        api: WindowsApi32::CloseHandle,
        name: "CloseHandle",
        stack_words: 1,
        introduced: ProviderProfile::Synchronization,
    },
    ApiDefinition {
        api: WindowsApi32::DuplicateHandle,
        name: "DuplicateHandle",
        stack_words: 7,
        introduced: ProviderProfile::Synchronization,
    },
    ApiDefinition {
        api: WindowsApi32::WaitForSingleObject,
        name: "WaitForSingleObject",
        stack_words: 2,
        introduced: ProviderProfile::Synchronization,
    },
    ApiDefinition {
        api: WindowsApi32::WaitForSingleObjectEx,
        name: "WaitForSingleObjectEx",
        stack_words: 3,
        introduced: ProviderProfile::Synchronization,
    },
    ApiDefinition {
        api: WindowsApi32::WaitForMultipleObjects,
        name: "WaitForMultipleObjects",
        stack_words: 4,
        introduced: ProviderProfile::Synchronization,
    },
    ApiDefinition {
        api: WindowsApi32::WaitForMultipleObjectsEx,
        name: "WaitForMultipleObjectsEx",
        stack_words: 5,
        introduced: ProviderProfile::Synchronization,
    },
    ApiDefinition {
        api: WindowsApi32::Sleep,
        name: "Sleep",
        stack_words: 1,
        introduced: ProviderProfile::Synchronization,
    },
    ApiDefinition {
        api: WindowsApi32::SleepEx,
        name: "SleepEx",
        stack_words: 2,
        introduced: ProviderProfile::Synchronization,
    },
    ApiDefinition {
        api: WindowsApi32::SignalObjectAndWait,
        name: "SignalObjectAndWait",
        stack_words: 4,
        introduced: ProviderProfile::Synchronization,
    },
    ApiDefinition {
        api: WindowsApi32::SetDllDirectoryA,
        name: "SetDllDirectoryA",
        stack_words: 1,
        introduced: ProviderProfile::Modules,
    },
    ApiDefinition {
        api: WindowsApi32::SetDllDirectoryW,
        name: "SetDllDirectoryW",
        stack_words: 1,
        introduced: ProviderProfile::Modules,
    },
    ApiDefinition {
        api: WindowsApi32::GetDllDirectoryA,
        name: "GetDllDirectoryA",
        stack_words: 2,
        introduced: ProviderProfile::Modules,
    },
    ApiDefinition {
        api: WindowsApi32::GetDllDirectoryW,
        name: "GetDllDirectoryW",
        stack_words: 2,
        introduced: ProviderProfile::Modules,
    },
];

impl WindowsApi32 {
    pub(crate) const CATALOG_LEN: usize = APIS.len();

    pub fn resolve(module: &str, symbol: &str) -> Option<Self> {
        if !module.eq_ignore_ascii_case("kernel32.dll") {
            return None;
        }
        APIS.iter()
            .find(|entry| entry.name == symbol)
            .map(|entry| entry.api)
    }

    pub fn from_id(id: u32) -> Option<Self> {
        let index = id.checked_sub(Self::GetLastError.id())? as usize;
        let entry = APIS.get(index)?;
        (entry.api.id() == id).then_some(entry.api)
    }

    pub fn id(self) -> u32 {
        self as u32
    }

    pub fn name(self) -> &'static str {
        APIS[self.catalog_index()].name
    }

    pub fn convention(self) -> CallingConvention32 {
        CallingConvention32::Stdcall
    }

    pub fn stack_words(self) -> u32 {
        APIS[self.catalog_index()].stack_words
    }

    pub(crate) const fn catalog_index(self) -> usize {
        match self {
            Self::GetLastError => 0,
            Self::SetLastError => 1,
            Self::ExitProcess => 2,
            Self::GetModuleHandleA => 3,
            Self::VirtualAlloc => 4,
            Self::VirtualFree => 5,
            Self::VirtualProtect => 6,
            Self::FlushInstructionCache => 7,
            Self::GetCurrentProcess => 8,
            Self::LoadLibraryA => 9,
            Self::LoadLibraryW => 10,
            Self::LoadLibraryExA => 11,
            Self::LoadLibraryExW => 12,
            Self::GetProcAddress => 13,
            Self::FreeLibrary => 14,
            Self::GetModuleHandleW => 15,
            Self::CreateEventA => 16,
            Self::CreateEventW => 17,
            Self::OpenEventA => 18,
            Self::OpenEventW => 19,
            Self::SetEvent => 20,
            Self::ResetEvent => 21,
            Self::PulseEvent => 22,
            Self::CloseHandle => 23,
            Self::DuplicateHandle => 24,
            Self::WaitForSingleObject => 25,
            Self::WaitForSingleObjectEx => 26,
            Self::WaitForMultipleObjects => 27,
            Self::WaitForMultipleObjectsEx => 28,
            Self::Sleep => 29,
            Self::SleepEx => 30,
            Self::SignalObjectAndWait => 31,
            Self::SetDllDirectoryA => 32,
            Self::SetDllDirectoryW => 33,
            Self::GetDllDirectoryA => 34,
            Self::GetDllDirectoryW => 35,
        }
    }

    pub(crate) fn catalog() -> impl ExactSizeIterator<Item = Self> {
        APIS.iter().map(|entry| entry.api)
    }
}

impl ProviderProfile {
    pub(crate) fn supports(self, api: WindowsApi32) -> bool {
        self as u8 >= APIS[api.catalog_index()].introduced as u8
    }
}
