#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum MemoryApi32 {
    GetProcessHeap = 0x000b_0001,
    HeapCreate,
    HeapDestroy,
    HeapAlloc,
    HeapReAlloc,
    HeapFree,
    HeapSize,
    HeapValidate,
    HeapWalk,
    GetProcessHeaps,
    HeapLock,
    HeapUnlock,
    HeapCompact,
    LocalAlloc,
    LocalReAlloc,
    LocalFree,
    LocalLock,
    LocalUnlock,
    LocalSize,
    LocalFlags,
    LocalHandle,
    GlobalAlloc,
    GlobalReAlloc,
    GlobalFree,
    GlobalLock,
    GlobalUnlock,
    GlobalSize,
    GlobalFlags,
    GlobalHandle,
    VirtualQuery,
    VirtualAllocEx,
    VirtualFreeEx,
    VirtualProtectEx,
    VirtualQueryEx,
    HeapSetInformation,
    HeapQueryInformation,
    InitializeSListHead,
}
impl MemoryApi32 {
    pub const ALL: [Self; 37] = [
        Self::GetProcessHeap,
        Self::HeapCreate,
        Self::HeapDestroy,
        Self::HeapAlloc,
        Self::HeapReAlloc,
        Self::HeapFree,
        Self::HeapSize,
        Self::HeapValidate,
        Self::HeapWalk,
        Self::GetProcessHeaps,
        Self::HeapLock,
        Self::HeapUnlock,
        Self::HeapCompact,
        Self::LocalAlloc,
        Self::LocalReAlloc,
        Self::LocalFree,
        Self::LocalLock,
        Self::LocalUnlock,
        Self::LocalSize,
        Self::LocalFlags,
        Self::LocalHandle,
        Self::GlobalAlloc,
        Self::GlobalReAlloc,
        Self::GlobalFree,
        Self::GlobalLock,
        Self::GlobalUnlock,
        Self::GlobalSize,
        Self::GlobalFlags,
        Self::GlobalHandle,
        Self::VirtualQuery,
        Self::VirtualAllocEx,
        Self::VirtualFreeEx,
        Self::VirtualProtectEx,
        Self::VirtualQueryEx,
        Self::HeapSetInformation,
        Self::HeapQueryInformation,
        Self::InitializeSListHead,
    ];
    pub fn from_id(id: u32) -> Option<Self> {
        Self::ALL
            .get(id.checked_sub(0x000b_0001)? as usize)
            .copied()
    }
    pub fn id(self) -> u32 {
        self as u32
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::GetProcessHeap => "GetProcessHeap",
            Self::HeapCreate => "HeapCreate",
            Self::HeapDestroy => "HeapDestroy",
            Self::HeapAlloc => "HeapAlloc",
            Self::HeapReAlloc => "HeapReAlloc",
            Self::HeapFree => "HeapFree",
            Self::HeapSize => "HeapSize",
            Self::HeapValidate => "HeapValidate",
            Self::HeapWalk => "HeapWalk",
            Self::GetProcessHeaps => "GetProcessHeaps",
            Self::HeapLock => "HeapLock",
            Self::HeapUnlock => "HeapUnlock",
            Self::HeapCompact => "HeapCompact",
            Self::LocalAlloc => "LocalAlloc",
            Self::LocalReAlloc => "LocalReAlloc",
            Self::LocalFree => "LocalFree",
            Self::LocalLock => "LocalLock",
            Self::LocalUnlock => "LocalUnlock",
            Self::LocalSize => "LocalSize",
            Self::LocalFlags => "LocalFlags",
            Self::LocalHandle => "LocalHandle",
            Self::GlobalAlloc => "GlobalAlloc",
            Self::GlobalReAlloc => "GlobalReAlloc",
            Self::GlobalFree => "GlobalFree",
            Self::GlobalLock => "GlobalLock",
            Self::GlobalUnlock => "GlobalUnlock",
            Self::GlobalSize => "GlobalSize",
            Self::GlobalFlags => "GlobalFlags",
            Self::GlobalHandle => "GlobalHandle",
            Self::VirtualQuery => "VirtualQuery",
            Self::VirtualAllocEx => "VirtualAllocEx",
            Self::VirtualFreeEx => "VirtualFreeEx",
            Self::VirtualProtectEx => "VirtualProtectEx",
            Self::VirtualQueryEx => "VirtualQueryEx",
            Self::HeapSetInformation => "HeapSetInformation",
            Self::HeapQueryInformation => "HeapQueryInformation",
            Self::InitializeSListHead => "InitializeSListHead",
        }
    }
    pub fn stack_words(self) -> u32 {
        match self {
            Self::GetProcessHeap => 0,
            Self::HeapDestroy
            | Self::HeapLock
            | Self::HeapUnlock
            | Self::LocalFree
            | Self::LocalLock
            | Self::LocalUnlock
            | Self::LocalSize
            | Self::LocalFlags
            | Self::LocalHandle
            | Self::GlobalFree
            | Self::GlobalLock
            | Self::GlobalUnlock
            | Self::GlobalSize
            | Self::GlobalFlags
            | Self::GlobalHandle
            | Self::InitializeSListHead => 1,
            Self::HeapWalk
            | Self::GetProcessHeaps
            | Self::HeapCompact
            | Self::LocalAlloc
            | Self::GlobalAlloc => 2,
            Self::HeapCreate
            | Self::HeapAlloc
            | Self::HeapFree
            | Self::HeapSize
            | Self::HeapValidate
            | Self::LocalReAlloc
            | Self::GlobalReAlloc
            | Self::VirtualQuery => 3,
            Self::HeapReAlloc
            | Self::VirtualFreeEx
            | Self::VirtualQueryEx
            | Self::HeapSetInformation => 4,
            Self::VirtualAllocEx | Self::VirtualProtectEx | Self::HeapQueryInformation => 5,
        }
    }
}
