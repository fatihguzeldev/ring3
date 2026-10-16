#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ThreadApi32 {
    CreateThread = 0x000f_0001,
    OpenThread,
    ExitThread,
    GetExitCodeThread,
    SuspendThread,
    ResumeThread,
    TerminateThread,
    SwitchToThread,
    TlsAlloc,
    TlsFree,
    TlsGetValue,
    TlsSetValue,
    DisableThreadLibraryCalls,
    GetThreadPriority,
    SetThreadPriority,
    GetThreadTimes,
    GetThreadContext,
    SetThreadContext,
    FlsAlloc,
    FlsFree,
    FlsGetValue,
    FlsSetValue,
}
impl ThreadApi32 {
    pub const ALL: [Self; 22] = [
        Self::CreateThread,
        Self::OpenThread,
        Self::ExitThread,
        Self::GetExitCodeThread,
        Self::SuspendThread,
        Self::ResumeThread,
        Self::TerminateThread,
        Self::SwitchToThread,
        Self::TlsAlloc,
        Self::TlsFree,
        Self::TlsGetValue,
        Self::TlsSetValue,
        Self::DisableThreadLibraryCalls,
        Self::GetThreadPriority,
        Self::SetThreadPriority,
        Self::GetThreadTimes,
        Self::GetThreadContext,
        Self::SetThreadContext,
        Self::FlsAlloc,
        Self::FlsFree,
        Self::FlsGetValue,
        Self::FlsSetValue,
    ];
    pub fn core_apis() -> &'static [Self] {
        &Self::ALL[..]
    }
    pub fn tls_apis() -> &'static [Self] {
        &Self::ALL[8..12]
    }
    pub fn from_id(id: u32) -> Option<Self> {
        Self::ALL
            .get(id.checked_sub(0x000f_0001)? as usize)
            .copied()
    }
    pub fn id(self) -> u32 {
        self as u32
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::CreateThread => "CreateThread",
            Self::OpenThread => "OpenThread",
            Self::ExitThread => "ExitThread",
            Self::GetExitCodeThread => "GetExitCodeThread",
            Self::SuspendThread => "SuspendThread",
            Self::ResumeThread => "ResumeThread",
            Self::TerminateThread => "TerminateThread",
            Self::SwitchToThread => "SwitchToThread",
            Self::TlsAlloc => "TlsAlloc",
            Self::TlsFree => "TlsFree",
            Self::TlsGetValue => "TlsGetValue",
            Self::TlsSetValue => "TlsSetValue",
            Self::DisableThreadLibraryCalls => "DisableThreadLibraryCalls",
            Self::GetThreadPriority => "GetThreadPriority",
            Self::SetThreadPriority => "SetThreadPriority",
            Self::GetThreadTimes => "GetThreadTimes",
            Self::GetThreadContext => "GetThreadContext",
            Self::SetThreadContext => "SetThreadContext",
            Self::FlsAlloc => "FlsAlloc",
            Self::FlsFree => "FlsFree",
            Self::FlsGetValue => "FlsGetValue",
            Self::FlsSetValue => "FlsSetValue",
        }
    }
    pub fn stack_words(self) -> u32 {
        match self {
            Self::CreateThread => 6,
            Self::OpenThread => 3,
            Self::ExitThread
            | Self::SuspendThread
            | Self::ResumeThread
            | Self::TlsFree
            | Self::TlsGetValue
            | Self::DisableThreadLibraryCalls
            | Self::GetThreadPriority
            | Self::FlsAlloc
            | Self::FlsFree
            | Self::FlsGetValue => 1,
            Self::GetExitCodeThread
            | Self::TerminateThread
            | Self::TlsSetValue
            | Self::SetThreadPriority
            | Self::FlsSetValue => 2,
            Self::GetThreadContext | Self::SetThreadContext => 2,
            Self::SwitchToThread | Self::TlsAlloc => 0,
            Self::GetThreadTimes => 5,
        }
    }
}
