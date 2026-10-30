use std::collections::{BTreeSet, VecDeque};

use crate::{
    cpu::x86::{
        decode::{DecodeError, DecodedInstruction, decode_one},
        ir::{BranchTarget, Operation},
    },
    memory::{AddressSpace, CodeSnapshot, GuestAddress, MemoryError},
};

mod scalar;

#[derive(Debug)]
pub enum CompileError {
    InvalidLimits,
    Decode(DecodeError),
    Memory(MemoryError),
}

#[derive(Debug)]
pub struct CompiledUnit {
    pub wasm: Vec<u8>,
    pub sources: Vec<CodeSnapshot>,
    pub entries: Vec<u32>,
    pub instruction_count: usize,
}

impl CompiledUnit {
    pub fn is_current(&self, memory: &AddressSpace) -> bool {
        self.sources
            .iter()
            .all(|source| memory.is_code_current(source))
    }
}

struct Block {
    pc: u32,
    instructions: Vec<DecodedInstruction>,
    gate: Option<u32>,
}

pub fn compile(
    memory: &AddressSpace,
    pc: GuestAddress,
    limit: usize,
) -> Result<CompiledUnit, CompileError> {
    compile_with_gates(memory, pc, limit, &[])
}

pub fn compile_with_gates(
    memory: &AddressSpace,
    pc: GuestAddress,
    limit: usize,
    gates: &[(GuestAddress, u32)],
) -> Result<CompiledUnit, CompileError> {
    if limit == 0 || limit > 4096 {
        return Err(CompileError::InvalidLimits);
    }
    let mut pending = VecDeque::from([pc.0]);
    let mut seen = BTreeSet::new();
    let mut blocks = Vec::new();
    let mut sources = Vec::new();
    let mut count = 0;
    while let Some(entry) = pending.pop_front() {
        if count >= limit || blocks.len() >= 128 || !seen.insert(entry) {
            continue;
        }
        let mut instructions = Vec::new();
        let mut gate = None;
        let mut current = entry;
        while count < limit && instructions.len() < 64 {
            let mut matches = gates.iter().filter(|(address, _)| address.0 == current);
            if let Some(&(_, id)) = matches.next() {
                if id == 0 || matches.next().is_some() {
                    return Err(CompileError::InvalidLimits);
                }
                let mut stub = [0; 2];
                memory
                    .fetch(GuestAddress(current), &mut stub)
                    .map_err(CompileError::Memory)?;
                if stub != [0x0f, 0x0b] {
                    return Err(CompileError::InvalidLimits);
                }
                sources.push(
                    memory
                        .snapshot_code(GuestAddress(current), 2)
                        .map_err(CompileError::Memory)?,
                );
                gate = Some(id);
                count += 1;
                break;
            }
            let instruction = match decode_one(memory, GuestAddress(current)) {
                Ok(instruction) => instruction,
                Err(error) if blocks.is_empty() && instructions.is_empty() => {
                    return Err(CompileError::Decode(error));
                }
                Err(_) => break,
            };
            let next = instruction.next_pc().0;
            let operation = *instruction.operation();
            sources.push(
                memory
                    .snapshot_code(instruction.pc(), instruction.length() as usize)
                    .map_err(|error| CompileError::Decode(DecodeError::Infrastructure(error)))?,
            );
            instructions.push(instruction);
            count += 1;
            let terminal = match operation {
                Operation::Jump {
                    target: BranchTarget::Direct(target),
                } => {
                    pending.push_back(target.0);
                    true
                }
                Operation::Call {
                    target: BranchTarget::Direct(target),
                } => {
                    pending.push_back(target.0);
                    pending.push_back(next);
                    true
                }
                Operation::ConditionalJump { target, .. }
                | Operation::CountBranch { target, .. } => {
                    pending.push_back(target.0);
                    pending.push_back(next);
                    true
                }
                Operation::Jump { .. } | Operation::Call { .. } | Operation::Return { .. } => true,
                _ => !scalar::supports(&operation),
            };
            current = next;
            if terminal {
                break;
            }
        }
        if !instructions.is_empty() || gate.is_some() {
            blocks.push(Block {
                pc: entry,
                instructions,
                gate,
            });
        }
    }
    let entries = blocks.iter().map(|block| block.pc).collect();
    Ok(CompiledUnit {
        wasm: scalar::emit(&blocks),
        sources,
        entries,
        instruction_count: count,
    })
}

#[cfg(test)]
mod string_tests;
#[cfg(test)]
mod tests;
