use crate::cpu::x86::State32;

pub const CONTEXT_SIZE: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum StopReason {
    NeedCode = 1,
    Budget = 2,
    MemoryFault = 3,
    Unsupported = 4,
    Debug = 5,
    CodeInvalidated = 6,
    Host = 7,
    Infrastructure = 8,
    CpuFault = 9,
    SystemEntry = 10,
    /// Private compiled ingress request. Public wrappers consume this reason.
    ExactPrefix = 11,
}

#[derive(Clone, Debug)]
#[repr(C, align(8))]
pub struct Context32 {
    bytes: [u8; CONTEXT_SIZE],
}

impl Default for Context32 {
    fn default() -> Self {
        Self::new(State32::default())
    }
}

impl Context32 {
    pub fn new(state: State32) -> Self {
        let mut context = Self {
            bytes: [0; CONTEXT_SIZE],
        };
        context.set_state(state);
        context
    }

    pub fn state(&self) -> State32 {
        State32 {
            registers: std::array::from_fn(|index| self.word(index * 4)),
            eip: self.word(32),
            eflags: self.word(36),
        }
    }

    pub fn set_state(&mut self, state: State32) {
        for (index, value) in state.registers.into_iter().enumerate() {
            self.set_word(index * 4, value);
        }
        self.set_word(32, state.eip);
        self.set_word(36, state.eflags | 2);
    }

    pub fn retired(&self) -> u32 {
        self.word(40)
    }
    pub fn remaining(&self) -> u32 {
        self.word(44)
    }
    pub fn bytes(&self) -> &[u8; CONTEXT_SIZE] {
        &self.bytes
    }
    pub(crate) fn bytes_mut(&mut self) -> &mut [u8; CONTEXT_SIZE] {
        &mut self.bytes
    }

    fn word(&self, offset: usize) -> u32 {
        u32::from_le_bytes(
            self.bytes[offset..offset + 4]
                .try_into()
                .expect("context word"),
        )
    }

    pub(crate) fn set_word(&mut self, offset: usize, value: u32) {
        self.bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
}
