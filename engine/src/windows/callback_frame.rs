use super::{FrameError, MAX_STACK_WORDS};
use crate::{
    cpu::x86::{Register32, State32},
    memory::{GuestAddress, WordWrite32},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallbackFrame32 {
    state: State32,
    writes: [WordWrite32; MAX_STACK_WORDS + 1],
    write_count: usize,
    return_esp: u32,
}

impl CallbackFrame32 {
    pub fn prepare(
        mut state: State32,
        entry_pc: u32,
        return_pc: u32,
        arguments: &[u32],
    ) -> Result<Self, FrameError> {
        if arguments.len() > MAX_STACK_WORDS {
            return Err(FrameError::InvalidRequest);
        }
        let return_esp = state.registers[Register32::Esp.index()];
        let write_count = arguments.len() + 1;
        let entry_esp = return_esp.wrapping_sub(4 * write_count as u32);
        let mut writes = [WordWrite32 {
            address: GuestAddress(0),
            value: 0,
        }; MAX_STACK_WORDS + 1];
        writes[0] = WordWrite32 {
            address: GuestAddress(entry_esp),
            value: return_pc,
        };
        for (index, value) in arguments.iter().enumerate() {
            writes[index + 1] = WordWrite32 {
                address: GuestAddress(entry_esp.wrapping_add(4 * (index as u32 + 1))),
                value: *value,
            };
        }
        state.eip = entry_pc;
        state.registers[Register32::Esp.index()] = entry_esp;
        Ok(Self {
            state,
            writes,
            write_count,
            return_esp,
        })
    }

    pub fn state(&self) -> &State32 {
        &self.state
    }

    pub fn writes(&self) -> &[WordWrite32] {
        &self.writes[..self.write_count]
    }

    pub fn return_esp(&self) -> u32 {
        self.return_esp
    }
}
