use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::{
    cpu::x86::{
        decode::{
            BoundDecode, DecodeAuthority, DecodeError, DecodePolicy, DecodedOperation,
            decode_for_compilation,
        },
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
    pub(crate) services: Vec<ServiceSite>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ServiceSite {
    pub(crate) pc: u32,
    pub(crate) next_pc: u32,
    pub(crate) operation: Operation,
}

fn can_service(operation: &Operation) -> bool {
    // inline numeric operations may still defer at runtime, even in a block's
    // interior; other unsupported operations retain the same strict service.
    !scalar::supports(operation)
        || matches!(
            operation,
            Operation::X87 { .. }
                | Operation::Simd(_)
                | Operation::Avx(_)
                | Operation::XState(_)
                | Operation::InitializeX87
                | Operation::ClearX87Exceptions
                | Operation::X87StatusToAx
                | Operation::X87StatusToMemory { .. }
                | Operation::X87ControlToMemory { .. }
        )
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
    instructions: Vec<DecodedOperation>,
    gate: Option<u32>,
}

struct EntryPoint {
    pc: u32,
    block: u32,
    instruction: u32,
}

fn entry_points(blocks: &[Block]) -> Vec<EntryPoint> {
    let mut entries = BTreeMap::new();
    for (block_index, block) in blocks.iter().enumerate() {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            entries
                .entry(instruction.pc().0)
                .or_insert((block_index, instruction_index));
        }
        if block.gate.is_some() {
            let pc = block
                .instructions
                .last()
                .map_or(block.pc, |instruction| instruction.next_pc().0);
            entries
                .entry(pc)
                .or_insert((block_index, block.instructions.len()));
        }
    }
    entries
        .into_iter()
        .map(|(pc, (block, instruction))| EntryPoint {
            pc,
            block: block as u32,
            instruction: instruction as u32,
        })
        .collect()
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
    compile_with_gates_until(memory, pc, limit, gates, |_| false)
}

/// compile a missing entry, stopping before PCs owned by current cached code.
/// coverage must remain stable during this call; the root must be uncovered.
pub(crate) fn compile_with_gates_until(
    memory: &AddressSpace,
    pc: GuestAddress,
    limit: usize,
    gates: &[(GuestAddress, u32)],
    covered: impl Fn(GuestAddress) -> bool,
) -> Result<CompiledUnit, CompileError> {
    if limit == 0 || limit > 4096 {
        return Err(CompileError::InvalidLimits);
    }
    if covered(pc) {
        return Err(CompileError::InvalidLimits);
    }
    let mut pending = VecDeque::from([pc.0]);
    let mut seen = BTreeSet::new();
    let mut blocks = Vec::new();
    let mut sources = Vec::new();
    let mut services = Vec::new();
    let mut count = 0;
    while let Some(entry) = pending.pop_front() {
        if count >= limit
            || blocks.len() >= 128
            || covered(GuestAddress(entry))
            || !seen.insert(entry)
        {
            continue;
        }
        let mut instructions = Vec::new();
        let mut gate = None;
        let mut current = entry;
        let mut source_length = 0;
        while count < limit && instructions.len() < 64 {
            if covered(GuestAddress(current)) {
                break;
            }
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
                source_length += 2;
                gate = Some(id);
                count += 1;
                break;
            }
            let instruction = match decode_for_compilation(
                memory,
                GuestAddress(current),
                DecodePolicy::Strict,
                None,
                DecodeAuthority::default(),
            ) {
                Ok(BoundDecode::Instruction(instruction)) => instruction,
                Ok(BoundDecode::CodeFault(_)) => return Err(CompileError::InvalidLimits),
                Err(error) if blocks.is_empty() && instructions.is_empty() => {
                    return Err(CompileError::Decode(error));
                }
                Err(_) => break,
            };
            let next = instruction.next_pc().0;
            let operation = *instruction.operation();
            if can_service(&operation) {
                services.push(ServiceSite {
                    pc: current,
                    next_pc: next,
                    operation,
                });
            }
            source_length += usize::from(instruction.length());
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
            sources.push(
                memory
                    .snapshot_code_wrapping(GuestAddress(entry), source_length)
                    .map_err(|error| {
                        if gate.is_some() {
                            CompileError::Memory(error)
                        } else {
                            CompileError::Decode(DecodeError::Infrastructure(error))
                        }
                    })?,
            );
            blocks.push(Block {
                pc: entry,
                instructions,
                gate,
            });
        }
    }
    let points = entry_points(&blocks);
    let entries = points.iter().map(|entry| entry.pc).collect();
    services.sort_unstable_by_key(|site| site.pc);
    services.dedup_by_key(|site| site.pc);
    Ok(CompiledUnit {
        wasm: scalar::emit(&blocks, &points),
        sources,
        entries,
        instruction_count: count,
        services,
    })
}

#[cfg(test)]
mod bit_tests;
#[cfg(test)]
mod coverage_tests;
#[cfg(test)]
mod exchange_tests;
#[cfg(test)]
mod flags_tests;
#[cfg(test)]
mod memory_tests;
#[cfg(test)]
mod multi_entry_tests;
#[cfg(test)]
mod multiply_tests;
#[cfg(test)]
mod rotate_tests;
#[cfg(test)]
mod service_tests;
#[cfg(test)]
mod shift_tests;
#[cfg(test)]
mod simd_tests;
#[cfg(test)]
mod source_tests;
#[cfg(test)]
mod string_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod x87_arithmetic_tests;
#[cfg(test)]
mod x87_divide_tests;
#[cfg(test)]
mod x87_multiply_tests;
#[cfg(test)]
mod x87_tests;
