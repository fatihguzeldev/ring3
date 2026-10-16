#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum NativeApi32 {
    LdrAddRefDll = 0x0017_0001,
    NtQuerySystemTime,
    VDbgPrintExWithPrefix,
}
impl NativeApi32 {
    pub const ALL: [Self; 3] = [
        Self::LdrAddRefDll,
        Self::NtQuerySystemTime,
        Self::VDbgPrintExWithPrefix,
    ];
    pub fn from_id(id: u32) -> Option<Self> {
        Self::ALL
            .get(id.checked_sub(0x0017_0001)? as usize)
            .copied()
    }
    pub fn id(self) -> u32 {
        self as u32
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::LdrAddRefDll => "LdrAddRefDll",
            Self::NtQuerySystemTime => "NtQuerySystemTime",
            Self::VDbgPrintExWithPrefix => "vDbgPrintExWithPrefix",
        }
    }
    pub fn stack_words(self) -> u32 {
        match self {
            Self::LdrAddRefDll => 2,
            Self::NtQuerySystemTime => 1,
            Self::VDbgPrintExWithPrefix => 5,
        }
    }
}
