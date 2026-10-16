#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum SynchronizationApi32 {
    CreateMutexA = 0x0010_0001,
    CreateMutexW,
    OpenMutexA,
    OpenMutexW,
    ReleaseMutex,
    CreateSemaphoreA,
    CreateSemaphoreW,
    OpenSemaphoreA,
    OpenSemaphoreW,
    ReleaseSemaphore,
    CreateWaitableTimerA,
    CreateWaitableTimerW,
    OpenWaitableTimerA,
    OpenWaitableTimerW,
    SetWaitableTimer,
    CancelWaitableTimer,
    InitializeCriticalSection,
    InitializeCriticalSectionAndSpinCount,
    DeleteCriticalSection,
    EnterCriticalSection,
    LeaveCriticalSection,
    TryEnterCriticalSection,
    SetCriticalSectionSpinCount,
    QueueUserAPC,
    InitializeCriticalSectionEx,
}
impl SynchronizationApi32 {
    pub fn object_apis() -> &'static [Self] {
        &Self::ALL[..]
    }
    pub const ALL: [Self; 25] = [
        Self::CreateMutexA,
        Self::CreateMutexW,
        Self::OpenMutexA,
        Self::OpenMutexW,
        Self::ReleaseMutex,
        Self::CreateSemaphoreA,
        Self::CreateSemaphoreW,
        Self::OpenSemaphoreA,
        Self::OpenSemaphoreW,
        Self::ReleaseSemaphore,
        Self::CreateWaitableTimerA,
        Self::CreateWaitableTimerW,
        Self::OpenWaitableTimerA,
        Self::OpenWaitableTimerW,
        Self::SetWaitableTimer,
        Self::CancelWaitableTimer,
        Self::InitializeCriticalSection,
        Self::InitializeCriticalSectionAndSpinCount,
        Self::DeleteCriticalSection,
        Self::EnterCriticalSection,
        Self::LeaveCriticalSection,
        Self::TryEnterCriticalSection,
        Self::SetCriticalSectionSpinCount,
        Self::QueueUserAPC,
        Self::InitializeCriticalSectionEx,
    ];
    pub fn from_id(id: u32) -> Option<Self> {
        Self::ALL
            .get(id.checked_sub(0x0010_0001)? as usize)
            .copied()
    }
    pub fn id(self) -> u32 {
        self as u32
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::CreateMutexA => "CreateMutexA",
            Self::CreateMutexW => "CreateMutexW",
            Self::OpenMutexA => "OpenMutexA",
            Self::OpenMutexW => "OpenMutexW",
            Self::ReleaseMutex => "ReleaseMutex",
            Self::CreateSemaphoreA => "CreateSemaphoreA",
            Self::CreateSemaphoreW => "CreateSemaphoreW",
            Self::OpenSemaphoreA => "OpenSemaphoreA",
            Self::OpenSemaphoreW => "OpenSemaphoreW",
            Self::ReleaseSemaphore => "ReleaseSemaphore",
            Self::CreateWaitableTimerA => "CreateWaitableTimerA",
            Self::CreateWaitableTimerW => "CreateWaitableTimerW",
            Self::OpenWaitableTimerA => "OpenWaitableTimerA",
            Self::OpenWaitableTimerW => "OpenWaitableTimerW",
            Self::SetWaitableTimer => "SetWaitableTimer",
            Self::CancelWaitableTimer => "CancelWaitableTimer",
            Self::InitializeCriticalSection => "InitializeCriticalSection",
            Self::InitializeCriticalSectionAndSpinCount => "InitializeCriticalSectionAndSpinCount",
            Self::DeleteCriticalSection => "DeleteCriticalSection",
            Self::EnterCriticalSection => "EnterCriticalSection",
            Self::LeaveCriticalSection => "LeaveCriticalSection",
            Self::TryEnterCriticalSection => "TryEnterCriticalSection",
            Self::SetCriticalSectionSpinCount => "SetCriticalSectionSpinCount",
            Self::QueueUserAPC => "QueueUserAPC",
            Self::InitializeCriticalSectionEx => "InitializeCriticalSectionEx",
        }
    }
    pub fn stack_words(self) -> u32 {
        match self {
            Self::CreateMutexA
            | Self::CreateMutexW
            | Self::OpenMutexA
            | Self::OpenMutexW
            | Self::OpenSemaphoreA
            | Self::OpenSemaphoreW
            | Self::CreateWaitableTimerA
            | Self::CreateWaitableTimerW
            | Self::OpenWaitableTimerA
            | Self::OpenWaitableTimerW
            | Self::ReleaseSemaphore
            | Self::QueueUserAPC
            | Self::InitializeCriticalSectionEx => 3,
            Self::CreateSemaphoreA | Self::CreateSemaphoreW => 4,
            Self::SetWaitableTimer => 6,
            Self::InitializeCriticalSectionAndSpinCount | Self::SetCriticalSectionSpinCount => 2,
            _ => 1,
        }
    }
}
