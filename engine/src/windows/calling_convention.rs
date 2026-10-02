use crate::{
    cpu::x86::{Register32, State32},
    memory::{AddressSpace, GuestAddress, MemoryError},
};

pub const MAX_STACK_WORDS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallingConvention32 {
    Cdecl,
    Stdcall,
    Thiscall,
}

impl TryFrom<u32> for CallingConvention32 {
    type Error = FrameError;

    fn try_from(tag: u32) -> Result<Self, Self::Error> {
        match tag {
            1 => Ok(Self::Cdecl),
            2 => Ok(Self::Stdcall),
            3 => Ok(Self::Thiscall),
            _ => Err(FrameError::InvalidRequest),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameError {
    InvalidRequest,
    Memory(MemoryError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallFrame32 {
    state: State32,
    convention: CallingConvention32,
    stack_words: u32,
    return_pc: u32,
    arguments: [u32; MAX_STACK_WORDS],
}

impl CallFrame32 {
    pub fn capture(
        memory: &AddressSpace,
        state: State32,
        convention: CallingConvention32,
        stack_words: u32,
    ) -> Result<Self, FrameError> {
        if stack_words > MAX_STACK_WORDS as u32 {
            return Err(FrameError::InvalidRequest);
        }
        let entry_esp = state.registers[Register32::Esp.index()];
        let return_pc = read_word(memory, entry_esp)?;
        let mut arguments = [0; MAX_STACK_WORDS];
        for (index, argument) in arguments[..stack_words as usize].iter_mut().enumerate() {
            let address = entry_esp.wrapping_add(4 * (index as u32 + 1));
            *argument = read_word(memory, address)?;
        }
        Ok(Self {
            state,
            convention,
            stack_words,
            return_pc,
            arguments,
        })
    }

    pub fn state(&self) -> &State32 {
        &self.state
    }

    pub fn convention(&self) -> CallingConvention32 {
        self.convention
    }

    pub fn stack_words(&self) -> u32 {
        self.stack_words
    }

    pub fn arguments(&self) -> &[u32] {
        &self.arguments[..self.stack_words as usize]
    }

    pub fn return_pc(&self) -> u32 {
        self.return_pc
    }

    pub fn this_pointer(&self) -> Option<u32> {
        (self.convention == CallingConvention32::Thiscall)
            .then_some(self.state.registers[Register32::Ecx.index()])
    }

    pub fn complete(&self, result: u32) -> State32 {
        let mut state = self.state;
        let cleanup = match self.convention {
            CallingConvention32::Cdecl => 0,
            CallingConvention32::Stdcall | CallingConvention32::Thiscall => 4 * self.stack_words,
        };
        state.registers[Register32::Eax.index()] = result;
        state.registers[Register32::Esp.index()] =
            state.registers[Register32::Esp.index()].wrapping_add(4 + cleanup);
        state.eip = self.return_pc;
        state
    }
}

fn read_word(memory: &AddressSpace, address: u32) -> Result<u32, FrameError> {
    let mut bytes = [0; 4];
    memory
        .read(GuestAddress(address), &mut bytes)
        .map_err(FrameError::Memory)?;
    Ok(u32::from_le_bytes(bytes))
}
