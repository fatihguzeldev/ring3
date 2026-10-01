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
