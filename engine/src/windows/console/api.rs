#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ConsoleApi32 {
    SetConsoleCtrlHandler = 0x0016_0001,
    VerifyConsoleIoHandle,
    WriteConsoleA,
    WriteConsoleW,
    GetConsoleMode,
    SetConsoleMode,
    GetNumberOfConsoleInputEvents,
    PeekConsoleInputA,
    ReadConsoleInputA,
    ReadConsoleA,
}
impl ConsoleApi32 {
    pub const ALL: [Self; 10] = [
        Self::SetConsoleCtrlHandler,
        Self::VerifyConsoleIoHandle,
        Self::WriteConsoleA,
        Self::WriteConsoleW,
        Self::GetConsoleMode,
        Self::SetConsoleMode,
        Self::GetNumberOfConsoleInputEvents,
        Self::PeekConsoleInputA,
        Self::ReadConsoleInputA,
        Self::ReadConsoleA,
    ];
    pub fn from_id(id: u32) -> Option<Self> {
        Self::ALL
            .get(id.checked_sub(0x0016_0001)? as usize)
            .copied()
    }
    pub fn id(self) -> u32 {
        self as u32
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::SetConsoleCtrlHandler => "SetConsoleCtrlHandler",
            Self::VerifyConsoleIoHandle => "VerifyConsoleIoHandle",
            Self::WriteConsoleA => "WriteConsoleA",
            Self::WriteConsoleW => "WriteConsoleW",
            Self::GetConsoleMode => "GetConsoleMode",
            Self::SetConsoleMode => "SetConsoleMode",
            Self::GetNumberOfConsoleInputEvents => "GetNumberOfConsoleInputEvents",
            Self::PeekConsoleInputA => "PeekConsoleInputA",
            Self::ReadConsoleInputA => "ReadConsoleInputA",
            Self::ReadConsoleA => "ReadConsoleA",
        }
    }
    pub fn stack_words(self) -> u32 {
        match self {
            Self::SetConsoleCtrlHandler
            | Self::GetConsoleMode
            | Self::SetConsoleMode
            | Self::GetNumberOfConsoleInputEvents => 2,
            Self::VerifyConsoleIoHandle => 1,
            Self::WriteConsoleA | Self::WriteConsoleW | Self::ReadConsoleA => 5,
            Self::PeekConsoleInputA | Self::ReadConsoleInputA => 4,
        }
    }
}
