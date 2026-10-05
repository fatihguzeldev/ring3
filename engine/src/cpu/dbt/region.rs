use crate::{
    cpu::x86::{
        decode::{DecodeError, DecodedInstruction, decode_one},
        ir::{BinaryKind, BranchTarget, Location32, Operation, SmallSource, UnaryKind, Value32},
    },
    memory::{AddressSpace, GuestAddress},
};

use super::gate::{self, GateSpec, PreparedGate};

pub(super) const MAX_BLOCKS: usize = 8;
const MAX_INSTRUCTIONS: usize = 64;
const MAX_WASM_BYTES: usize = 65_536;
pub(super) const GUEST_END: u64 = 1 << 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockSpec {
    pub entry: GuestAddress,
    pub byte_length: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompileLimits {
    pub blocks: usize,
    pub instructions: usize,
    pub wasm_bytes: usize,
}

impl Default for CompileLimits {
    fn default() -> Self {
        Self {
            blocks: MAX_BLOCKS,
            instructions: MAX_INSTRUCTIONS,
            wasm_bytes: MAX_WASM_BYTES,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompileError {
    InvalidLimits,
    InvalidBlocks,
    InvalidGates,
    Instruction {
        pc: GuestAddress,
        cause: InstructionError,
    },
    InstructionLimit,
    WasmLimit,
    Allocation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstructionError {
    Decode(DecodeError),
    BackendUnsupported,
    InvalidBlockEnd,
    InvalidGate,
}

#[derive(Debug)]
pub struct PreparedRegion {
    pub(super) blocks: Vec<CompiledBlock>,
}

#[derive(Clone, Copy)]
enum PreparationProfile {
    Standalone,
    Embedded,
    Resident,
}

#[derive(Debug)]
pub(super) struct CompiledBlock {
    pub(super) instructions: Vec<DecodedInstruction>,
    pub(super) gate: Option<PreparedGate>,
}

impl PreparedRegion {
    pub fn is_current(&self, memory: &AddressSpace) -> bool {
        self.blocks.iter().all(|block| {
            block
                .gate
                .as_ref()
                .is_none_or(|gate| gate.is_current(memory))
                && block
                    .instructions
                    .iter()
                    .all(|instruction| memory.is_code_current(instruction.code_snapshot()))
        })
    }

    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    pub fn instruction_count(&self) -> usize {
        self.blocks
            .iter()
            .map(|block| block.instructions.len() + usize::from(block.gate.is_some()))
            .sum()
    }
}

pub fn prepare_region(
    memory: &AddressSpace,
    specs: &[BlockSpec],
    limits: CompileLimits,
) -> Result<PreparedRegion, CompileError> {
    prepare(memory, specs, limits, PreparationProfile::Standalone, &[])
}

pub(super) fn prepare_resident_region(
    memory: &AddressSpace,
    specs: &[BlockSpec],
    limits: CompileLimits,
    gates: &[GateSpec],
) -> Result<PreparedRegion, CompileError> {
    prepare(memory, specs, limits, PreparationProfile::Resident, gates)
}

pub(super) fn prepare_embedded_region(
    memory: &AddressSpace,
    specs: &[BlockSpec],
    limits: CompileLimits,
    gates: &[GateSpec],
) -> Result<PreparedRegion, CompileError> {
    prepare(memory, specs, limits, PreparationProfile::Embedded, gates)
}

fn prepare(
    memory: &AddressSpace,
    specs: &[BlockSpec],
    limits: CompileLimits,
    profile: PreparationProfile,
    gates: &[GateSpec],
) -> Result<PreparedRegion, CompileError> {
    validate_limits(limits)?;
    validate_blocks(specs, limits.blocks)?;
    gate::validate(specs, gates)?;
    let embedded = matches!(profile, PreparationProfile::Embedded);
    let resident = matches!(profile, PreparationProfile::Resident);

    let mut blocks = Vec::new();
    blocks
        .try_reserve_exact(specs.len())
        .map_err(|_| CompileError::Allocation)?;
    let mut instruction_count = 0;

    for spec in specs {
        if let Some(gate) = gates.iter().find(|gate| gate.entry == spec.entry) {
            if instruction_count == limits.instructions {
                return Err(CompileError::InstructionLimit);
            }
            let gate = gate::prepare(memory, *gate)?;
            blocks.push(CompiledBlock {
                instructions: Vec::new(),
                gate: Some(gate),
            });
            instruction_count += 1;
            continue;
        }
        let mut instructions = Vec::new();
        let mut cursor = u64::from(spec.entry.0);
        let end = cursor + u64::from(spec.byte_length);
        while cursor < end {
            if instruction_count == limits.instructions {
                return Err(CompileError::InstructionLimit);
            }
            let pc = GuestAddress(cursor as u32);
            let instruction = decode_one(memory, pc)
                .map_err(|error| instruction_error(pc, InstructionError::Decode(error)))?;
            let next = cursor + u64::from(instruction.length());
            let terminates = terminates(&instruction, embedded)
                || (resident && supports_near_control(&instruction));
            if next > end || (terminates && next != end) {
                return Err(instruction_error(pc, InstructionError::InvalidBlockEnd));
            }
            if !(supports(&instruction, embedded)
                || (resident
                    && (supports_memory_reads(instruction.operation())
                        || supports_indirect_jump(instruction.operation())
                        || supports_memory_move_store(instruction.operation())
                        || supports_byte_store(instruction.operation())
                        || supports_memory_unary(instruction.operation())
                        || supports_memory_binary(instruction.operation())
                        || supports_memory_shift(instruction.operation())
                        || supports_stack_values(instruction.operation())
                        || supports_near_control(&instruction))))
            {
                return Err(instruction_error(pc, InstructionError::BackendUnsupported));
            }
            instructions
                .try_reserve(1)
                .map_err(|_| CompileError::Allocation)?;
            instructions.push(instruction);
            instruction_count += 1;
            cursor = next;
        }
        blocks.push(CompiledBlock {
            instructions,
            gate: None,
        });
    }

    Ok(PreparedRegion { blocks })
}

pub(super) fn validate_limits(limits: CompileLimits) -> Result<(), CompileError> {
    if limits.blocks == 0
        || limits.blocks > MAX_BLOCKS
        || limits.instructions == 0
        || limits.instructions > MAX_INSTRUCTIONS
        || limits.wasm_bytes == 0
        || limits.wasm_bytes > MAX_WASM_BYTES
    {
        return Err(CompileError::InvalidLimits);
    }
    Ok(())
}

pub(super) fn validate_blocks(specs: &[BlockSpec], limit: usize) -> Result<(), CompileError> {
    if specs.is_empty() || specs.len() > limit {
        return Err(CompileError::InvalidBlocks);
    }
    for (index, spec) in specs.iter().enumerate() {
        let start = u64::from(spec.entry.0);
        let end = start + u64::from(spec.byte_length);
        if spec.byte_length == 0 || end > GUEST_END {
            return Err(CompileError::InvalidBlocks);
        }
        for earlier in &specs[..index] {
            let earlier_start = u64::from(earlier.entry.0);
            let earlier_end = earlier_start + u64::from(earlier.byte_length);
            if start < earlier_end && earlier_start < end {
                return Err(CompileError::InvalidBlocks);
            }
        }
    }
    Ok(())
}

pub(super) fn instruction_error(pc: GuestAddress, cause: InstructionError) -> CompileError {
    CompileError::Instruction { pc, cause }
}

fn supports_stack_values(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::Push { .. } | Operation::Pop { .. } | Operation::Leave
    )
}

fn supports_near_control(instruction: &DecodedInstruction) -> bool {
    match instruction.operation() {
        Operation::Call {
            target: BranchTarget::Direct(_),
        } => instruction.length() == 5,
        Operation::Call {
            target: BranchTarget::Indirect(_),
        } => true,
        Operation::Return { .. } => matches!(instruction.length(), 1 | 3),
        _ => false,
    }
}

fn supports_stack(instruction: &DecodedInstruction) -> bool {
    supports_near_control(instruction) || supports_stack_values(instruction.operation())
}

pub(super) fn terminates(instruction: &DecodedInstruction, embedded: bool) -> bool {
    matches!(
        instruction.operation(),
        Operation::Jump { .. } | Operation::ConditionalJump { .. }
    ) || (embedded
        && supports_stack(instruction)
        && matches!(
            instruction.operation(),
            Operation::Call { .. } | Operation::Return { .. }
        ))
}

fn supports_memory_reads(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::LoadByte { .. }
            | Operation::MemoryPredicateByte { .. }
            | Operation::ReadCompareByte { .. }
            | Operation::ReadLogicalByte { .. }
            | Operation::ReadArithmeticByte { .. }
            | Operation::Move {
                destination: Location32::Register(_),
                source: Value32::Memory(_),
            }
            | Operation::Extend {
                source: SmallSource::Memory { .. },
                ..
            }
            | Operation::Binary {
                kind: BinaryKind::Add
                    | BinaryKind::Adc
                    | BinaryKind::Sub
                    | BinaryKind::Sbb
                    | BinaryKind::Cmp
                    | BinaryKind::And
                    | BinaryKind::Or
                    | BinaryKind::Xor,
                destination: Location32::Register(_),
                source: Value32::Memory(_),
            }
            | Operation::Binary {
                kind: BinaryKind::Cmp | BinaryKind::Test,
                destination: Location32::Memory(_),
                source: Value32::Register(_) | Value32::Immediate(_),
            }
            | Operation::SignedMultiply {
                source: Location32::Memory(_),
                ..
            }
    )
}

fn supports_indirect_jump(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::Jump {
            target: BranchTarget::Indirect(_),
        }
    )
}

fn supports_memory_move_store(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::Move {
            destination: Location32::Memory(_),
            source: Value32::Register(_) | Value32::Immediate(_),
        }
    )
}

fn supports_byte_store(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::StoreByte { .. } | Operation::MemorySetByte { .. }
    )
}

fn supports_memory_unary(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::MemoryUnaryByte { .. }
            | Operation::Unary {
                kind: UnaryKind::Inc | UnaryKind::Dec | UnaryKind::Not | UnaryKind::Neg,
                destination: Location32::Memory(_),
            }
    )
}

fn supports_memory_binary(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::MemoryLogicalByte { .. }
            | Operation::MemoryArithmeticByte { .. }
            | Operation::Binary {
                kind: BinaryKind::Add
                    | BinaryKind::Adc
                    | BinaryKind::Sub
                    | BinaryKind::Sbb
                    | BinaryKind::And
                    | BinaryKind::Or
                    | BinaryKind::Xor,
                destination: Location32::Memory(_),
                source: Value32::Register(_) | Value32::Immediate(_),
            }
    )
}

fn supports_memory_shift(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::MemoryShiftByte { .. }
            | Operation::Shift {
                destination: Location32::Memory(_),
                ..
            }
    )
}

pub(super) fn supports(instruction: &DecodedInstruction, embedded: bool) -> bool {
    let operation = instruction.operation();
    (embedded
        && (supports_memory_reads(operation)
            || supports_memory_move_store(operation)
            || supports_byte_store(operation)
            || supports_memory_unary(operation)
            || supports_memory_binary(operation)
            || supports_memory_shift(operation)
            || supports_stack(instruction)
            || supports_indirect_jump(operation)))
        || matches!(
            operation,
            Operation::Nop
                | Operation::MoveByte { .. }
                | Operation::CompareByte { .. }
                | Operation::TestByte { .. }
                | Operation::LogicalByte { .. }
                | Operation::ArithmeticByte { .. }
                | Operation::UnaryByte { .. }
                | Operation::ShiftByte { .. }
                | Operation::SetByte { .. }
                | Operation::Move {
                    destination: Location32::Register(_),
                    source: Value32::Register(_) | Value32::Immediate(_),
                }
                | Operation::Lea { .. }
                | Operation::Extend {
                    source: SmallSource::Register { .. },
                    ..
                }
                | Operation::Unary {
                    kind: UnaryKind::Inc | UnaryKind::Dec | UnaryKind::Not | UnaryKind::Neg,
                    destination: Location32::Register(_),
                }
                | Operation::Shift {
                    destination: Location32::Register(_),
                    ..
                }
                | Operation::SignedMultiply {
                    source: Location32::Register(_),
                    ..
                }
                | Operation::Binary {
                    kind: BinaryKind::Add
                        | BinaryKind::Adc
                        | BinaryKind::Sub
                        | BinaryKind::Sbb
                        | BinaryKind::Cmp
                        | BinaryKind::And
                        | BinaryKind::Or
                        | BinaryKind::Xor
                        | BinaryKind::Test,
                    destination: Location32::Register(_),
                    source: Value32::Register(_) | Value32::Immediate(_),
                }
                | Operation::Jump {
                    target: BranchTarget::Direct(_),
                }
                | Operation::ConditionalJump { .. }
        )
}
