use std::collections::{BTreeMap, BTreeSet, VecDeque};
use wasm_encoder::{NameMap, NameSection, Section};

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
    let mut wasm = scalar::emit(&blocks, &points);
    let mut functions = NameMap::new();
    functions.append(2, &format!("ring3_unit_{:08x}", pc.0));
    let mut names = NameSection::new();
    names.functions(&functions);
    let mut section = Vec::new();
    names.append_to(&mut section);
    wasm.reserve_exact(section.len());
    wasm.extend_from_slice(&section);
    Ok(CompiledUnit {
        wasm,
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

#[cfg(test)]
mod unit_name_tests {
    use super::*;
    use crate::memory::{PageRange, Permissions};

    fn take<'a>(bytes: &mut &'a [u8], length: usize) -> &'a [u8] {
        let (value, rest) = bytes.split_at(length);
        *bytes = rest;
        value
    }

    fn unsigned(bytes: &mut &[u8]) -> u32 {
        let mut value = 0;
        for shift in (0..35).step_by(7) {
            let byte = take(bytes, 1)[0];
            assert!(shift < 28 || byte < 16);
            value |= u32::from(byte & 127) << shift;
            if byte < 128 {
                return value;
            }
        }
        panic!("invalid unsigned LEB128");
    }

    fn name(bytes: &mut &[u8]) -> String {
        let length = unsigned(bytes) as usize;
        std::str::from_utf8(take(bytes, length)).unwrap().into()
    }

    #[test]
    fn generated_run_name_identifies_requested_entry_without_changing_export_index() {
        for (pc, expected_name) in [
            (0x0040_1040u32, "ring3_unit_00401040"),
            (0xffff_f000, "ring3_unit_fffff000"),
        ] {
            let mut memory = AddressSpace::new(1).unwrap();
            let base = pc & !4095;
            memory
                .map_zeroed(
                    PageRange::new(GuestAddress(base), 1).unwrap(),
                    Permissions::ALL,
                )
                .unwrap();
            memory
                .write(GuestAddress(base), &[0x90, 0xeb, 0xfd])
                .unwrap();
            if pc != base {
                memory.write(GuestAddress(pc), &[0xeb, 0xbe]).unwrap();
            }
            let unit = compile(&memory, GuestAddress(pc), 3).unwrap();
            assert_eq!(unit.entries[0], base);
            assert_eq!(&unit.wasm[..8], b"\0asm\x01\0\0\0");
            let mut bytes = &unit.wasm[8..];
            let mut imports = false;
            let mut run = None;
            let mut run_name = None;
            while !bytes.is_empty() {
                let id = take(&mut bytes, 1)[0];
                let length = unsigned(&mut bytes) as usize;
                let mut section = take(&mut bytes, length);
                match id {
                    2 => {
                        assert!(!imports);
                        imports = true;
                        assert_eq!(unsigned(&mut section), 3);
                        assert_eq!(name(&mut section), "env");
                        assert_eq!(name(&mut section), "memory");
                        assert_eq!(take(&mut section, 1), [2]);
                        assert_eq!(unsigned(&mut section), 0);
                        assert_eq!(unsigned(&mut section), 1);
                        for (function, index) in [("read", 0), ("write", 1)] {
                            assert_eq!(name(&mut section), "cpu");
                            assert_eq!(name(&mut section), function);
                            assert_eq!(take(&mut section, 1), [0]);
                            assert_eq!(unsigned(&mut section), index);
                        }
                        assert!(section.is_empty());
                    }
                    0 if name(&mut section) == "name" => {
                        assert!(run_name.is_none());
                        assert_eq!(take(&mut section, 1), [1]);
                        let length = unsigned(&mut section) as usize;
                        let mut functions = take(&mut section, length);
                        assert_eq!(unsigned(&mut functions), 1);
                        let index = unsigned(&mut functions);
                        run_name = Some((index, name(&mut functions)));
                        assert!(functions.is_empty() && section.is_empty());
                    }
                    7 => {
                        assert!(run.is_none());
                        assert_eq!(unsigned(&mut section), 1);
                        assert_eq!(name(&mut section), "run");
                        assert_eq!(take(&mut section, 1), [0]);
                        run = Some(unsigned(&mut section));
                        assert!(section.is_empty());
                    }
                    _ => {}
                }
            }
            assert!(imports);
            assert_eq!(run, Some(2));
            assert_eq!(run_name, Some((2, expected_name.into())));
        }
    }
}
