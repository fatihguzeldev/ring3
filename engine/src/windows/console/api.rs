#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ConsoleApi32 {
    SetConsoleCtrlHandler = 0x0016_0001,
    VerifyConsoleIoHandle,
    WriteConsoleA,
    WriteConsoleW,
}
impl ConsoleApi32 {
    pub const ALL: [Self; 4] = [
        Self::SetConsoleCtrlHandler,
        Self::VerifyConsoleIoHandle,
        Self::WriteConsoleA,
        Self::WriteConsoleW,
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
        }
    }
    pub fn stack_words(self) -> u32 {
        match self {
            Self::SetConsoleCtrlHandler => 2,
            Self::VerifyConsoleIoHandle => 1,
            Self::WriteConsoleA | Self::WriteConsoleW => 5,
        }
    }
}
