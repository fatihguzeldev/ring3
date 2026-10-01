#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State32 {
    // register order is EAX, ECX, EDX, EBX, ESP, EBP, ESI, EDI.
    pub registers: [u32; 8],
    pub eip: u32,
    pub eflags: u32,
}

impl Default for State32 {
    fn default() -> Self {
        Self {
            registers: [0; 8],
            eip: 0,
            eflags: 2,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Register32 {
    Eax,
    Ecx,
    Edx,
    Ebx,
    Esp,
    Ebp,
    Esi,
    Edi,
}

impl Register32 {
    pub const fn index(self) -> usize {
        match self {
            Self::Eax => 0,
            Self::Ecx => 1,
            Self::Edx => 2,
            Self::Ebx => 3,
            Self::Esp => 4,
            Self::Ebp => 5,
            Self::Esi => 6,
            Self::Edi => 7,
        }
    }
}
