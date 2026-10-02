use crate::{
    cpu::x86::decode::DecodeError,
    memory::{AddressSpace, CodeSnapshot, GuestAddress, MemoryError},
};

use super::{BlockSpec, CompileError, InstructionError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GateSpec {
    pub entry: GuestAddress,
    pub id: u32,
}

#[derive(Debug)]
pub(super) struct PreparedGate {
    pub(super) entry: GuestAddress,
    pub(super) id: u32,
    snapshot: CodeSnapshot,
}

impl PreparedGate {
    pub(super) fn is_current(&self, memory: &AddressSpace) -> bool {
        memory.is_code_current(&self.snapshot)
    }
}

pub(super) fn validate(blocks: &[BlockSpec], gates: &[GateSpec]) -> Result<(), CompileError> {
    if gates.len() > 8 || gates.len() > blocks.len() {
        return Err(CompileError::InvalidGates);
    }
    for (index, gate) in gates.iter().enumerate() {
        if gate.id == 0
            || !blocks
                .iter()
                .any(|block| block.entry == gate.entry && block.byte_length == 2)
            || gates[..index]
                .iter()
                .any(|earlier| earlier.entry == gate.entry || earlier.id == gate.id)
        {
            return Err(CompileError::InvalidGates);
        }
    }
    Ok(())
}

pub(super) fn prepare(memory: &AddressSpace, gate: GateSpec) -> Result<PreparedGate, CompileError> {
    let mut bytes = [0; 2];
    memory
        .fetch(gate.entry, &mut bytes)
        .map_err(|error| memory_error(gate.entry, error))?;
    if bytes != [0x0f, 0x0b] {
        return Err(CompileError::Instruction {
            pc: gate.entry,
            cause: InstructionError::InvalidGate,
        });
    }
    let snapshot = memory
        .snapshot_code(gate.entry, bytes.len())
        .map_err(|error| memory_error(gate.entry, error))?;
    Ok(PreparedGate {
        entry: gate.entry,
        id: gate.id,
        snapshot,
    })
}

fn memory_error(pc: GuestAddress, error: MemoryError) -> CompileError {
    let error = match error {
        MemoryError::Fault(fault) => DecodeError::MemoryFault {
            pc,
            fault,
            length: 2,
        },
        other => DecodeError::Infrastructure(other),
    };
    CompileError::Instruction {
        pc,
        cause: InstructionError::Decode(error),
    }
}
