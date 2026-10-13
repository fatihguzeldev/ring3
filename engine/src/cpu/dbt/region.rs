use std::sync::atomic::{AtomicU64, Ordering};

use crate::{
    cpu::x86::{
        decode::{
            BoundDecode, CapturedCodeFault, CodeFetchContext, DecodeError, DecodePolicy,
            DecodedInstruction, decode_bound, decode_one,
        },
        ir::{BinaryKind, BranchTarget, Location32, Operation, SmallSource, UnaryKind, Value32},
    },
    memory::{AddressSpace, CodeSnapshot, GuestAddress, MemoryError},
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
    memory_identity: u64,
    validated_version: AtomicU64,
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
    pub(super) source: BlockSource,
}

#[derive(Debug)]
pub(super) enum BlockSource {
    Instructions(CodeSnapshot),
    Gate(PreparedGate),
    ContextFault {
        fault: CapturedCodeFault,
        consumed: Option<CodeSnapshot>,
    },
}

impl CompiledBlock {
    pub(super) fn gate(&self) -> Option<&PreparedGate> {
        if let BlockSource::Gate(gate) = &self.source {
            Some(gate)
        } else {
            None
        }
    }
    pub(super) fn context_fault(&self) -> Option<&CapturedCodeFault> {
        if let BlockSource::ContextFault { fault, .. } = &self.source {
            Some(fault)
        } else {
            None
        }
    }
}

impl PreparedRegion {
    pub(super) fn new(memory: &AddressSpace, blocks: Vec<CompiledBlock>) -> Self {
        Self {
            blocks,
            memory_identity: memory.identity(),
            validated_version: AtomicU64::new(0),
        }
    }

    pub fn is_current(&self, memory: &AddressSpace) -> bool {
        if memory.identity() != self.memory_identity {
            return false;
        }
        let version = memory.code_validation_version();
        // non-executable writes leave every executable snapshot unchanged.
        if self.validated_version.load(Ordering::Relaxed) == version {
            return true;
        }
        let current = self.blocks.iter().all(|block| match &block.source {
            BlockSource::Gate(gate) => gate.is_current(memory),
            BlockSource::Instructions(snapshot) => memory.is_code_current(snapshot),
            BlockSource::ContextFault { consumed, .. } => consumed
                .as_ref()
                .is_none_or(|snapshot| memory.is_code_current(snapshot)),
        });
        if current {
            self.validated_version.store(version, Ordering::Relaxed);
        }
        current
    }

    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    pub fn instruction_count(&self) -> usize {
        self.blocks
            .iter()
            .map(|block| {
                block.instructions.len()
                    + usize::from(block.gate().is_some())
                    + usize::from(block.context_fault().is_some())
            })
            .sum()
    }
}

pub fn prepare_region(
    memory: &AddressSpace,
    specs: &[BlockSpec],
    limits: CompileLimits,
) -> Result<PreparedRegion, CompileError> {
    prepare(
        memory,
        specs,
        limits,
        PreparationProfile::Standalone,
        &[],
        None,
    )
}

pub(super) fn prepare_resident_region(
    memory: &AddressSpace,
    specs: &[BlockSpec],
    limits: CompileLimits,
    gates: &[GateSpec],
    cs: Option<CodeFetchContext>,
) -> Result<PreparedRegion, CompileError> {
    prepare(
        memory,
        specs,
        limits,
        PreparationProfile::Resident,
        gates,
        cs,
    )
}

pub(super) fn prepare_embedded_region(
    memory: &AddressSpace,
    specs: &[BlockSpec],
    limits: CompileLimits,
    gates: &[GateSpec],
    cs: Option<CodeFetchContext>,
) -> Result<PreparedRegion, CompileError> {
    prepare(
        memory,
        specs,
        limits,
        PreparationProfile::Embedded,
        gates,
        cs,
    )
}

fn prepare(
    memory: &AddressSpace,
    specs: &[BlockSpec],
    limits: CompileLimits,
    profile: PreparationProfile,
    gates: &[GateSpec],
    cs: Option<CodeFetchContext>,
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
            let gate = gate::prepare(memory, *gate, cs)?;
            blocks.push(CompiledBlock {
                instructions: Vec::new(),
                source: BlockSource::Gate(gate),
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
            let instruction = strict_instruction(memory, pc, cs)
                .map_err(|error| instruction_error(pc, InstructionError::Decode(error)))?;
            let next = cursor + u64::from(instruction.length());
            let terminates = terminates(&instruction, embedded)
                || (resident && supports_near_control(&instruction));
            if next > end || (terminates && next != end) {
                return Err(instruction_error(pc, InstructionError::InvalidBlockEnd));
            }
            if !(supports(&instruction, embedded)
                || (resident
                    && (supports_cpu_state(instruction.operation())
                        || supports_memory_reads(instruction.operation())
                        || supports_indirect_jump(instruction.operation())
                        || supports_memory_move_store(instruction.operation())
                        || supports_narrow_store(instruction.operation())
                        || supports_memory_unary(instruction.operation())
                        || supports_memory_binary(instruction.operation())
                        || supports_memory_shift_or_rotate(instruction.operation())
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
            source: BlockSource::Instructions(prepare_block_snapshot(
                memory,
                spec.entry,
                (cursor - u64::from(spec.entry.0)) as usize,
            )?),
            instructions,
        });
    }

    Ok(PreparedRegion::new(memory, blocks))
}

pub(super) fn strict_instruction(
    memory: &AddressSpace,
    pc: GuestAddress,
    cs: Option<CodeFetchContext>,
) -> Result<DecodedInstruction, DecodeError> {
    if let Some(cs) = cs {
        match decode_bound(memory, pc, cs, DecodePolicy::Strict)? {
            BoundDecode::Instruction(instruction) => Ok(instruction),
            BoundDecode::CodeFault(_) => unreachable!("strict decoding returns a typed error"),
        }
    } else {
        decode_one(memory, pc)
    }
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

pub(super) fn prepare_block_snapshot(
    memory: &AddressSpace,
    entry: GuestAddress,
    length: usize,
) -> Result<CodeSnapshot, CompileError> {
    memory.snapshot_code(entry, length).map_err(|error| {
        let error = match error {
            MemoryError::Allocation => return CompileError::Allocation,
            MemoryError::Fault(fault) => DecodeError::MemoryFault {
                pc: entry,
                fault,
                length: length as u32,
            },
            other => DecodeError::Infrastructure(other),
        };
        instruction_error(entry, InstructionError::Decode(error))
    })
}

fn supports_stack_values(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::Push { .. }
            | Operation::PushWord { .. }
            | Operation::PushMemoryWord { .. }
            | Operation::PushFlags
            | Operation::PushFlagsWord
            | Operation::Pop { .. }
            | Operation::PopWord { .. }
            | Operation::PopMemoryWord { .. }
            | Operation::PopAll
            | Operation::PopAllWord
            | Operation::EnterZero
            | Operation::EnterZeroWord
            | Operation::Leave
            | Operation::LeaveWord
    )
}

fn supports_near_control(instruction: &DecodedInstruction) -> bool {
    matches!(
        instruction.operation(),
        Operation::Call { .. }
            | Operation::CallRegisterWord { .. }
            | Operation::ReadCallWord { .. }
            | Operation::Return { .. }
            | Operation::CallWord { .. }
            | Operation::ReturnWord { .. }
    )
}

fn supports_stack(instruction: &DecodedInstruction) -> bool {
    supports_near_control(instruction) || supports_stack_values(instruction.operation())
}

pub(super) fn terminates(instruction: &DecodedInstruction, embedded: bool) -> bool {
    if matches!(
        instruction.operation(),
        Operation::Control(
            crate::cpu::x86::ControlOperation::InvalidOpcode
                | crate::cpu::x86::ControlOperation::GeneralProtectionZero
                | crate::cpu::x86::ControlOperation::Breakpoint
                | crate::cpu::x86::ControlOperation::DebugTrap
                | crate::cpu::x86::ControlOperation::DeniedPortString { .. }
        )
    ) {
        return true;
    }
    matches!(
        instruction.operation(),
        Operation::FarJump { .. }
            | Operation::FarCall { .. }
            | Operation::FarReturn { .. }
            | Operation::InterruptReturn { .. }
            | Operation::Jump { .. }
            | Operation::JumpWord { .. }
            | Operation::ReadJumpWord { .. }
            | Operation::ConditionalJump { .. }
            | Operation::CountBranch { .. }
            | Operation::CountBranchWord { .. }
    ) || (embedded
        && supports_stack(instruction)
        && matches!(
            instruction.operation(),
            Operation::Call { .. }
                | Operation::CallWord { .. }
                | Operation::CallRegisterWord { .. }
                | Operation::ReadCallWord { .. }
                | Operation::Return { .. }
                | Operation::ReturnWord { .. }
        ))
}

fn supports_cpu_state(operation: &Operation) -> bool {
    match operation {
        Operation::PackedStringCompare(operation) => operation.valid(),
        Operation::VirtualTimestampAux
        | Operation::FarJump { .. }
        | Operation::FarCall { .. }
        | Operation::FarReturn { .. }
        | Operation::InterruptReturn { .. }
        | Operation::XState(_)
        | Operation::Avx(_)
        | Operation::VerifySegment { .. }
        | Operation::QuerySegment { .. } => true,
        Operation::AdjustPrivilege { .. } => true,
        _ => false,
    }
}

fn supports_memory_reads(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::AdjustPrivilege {
            destination: Location32::Memory(_),
            ..
        } | Operation::CacheFlush { .. }
            | Operation::Crc32 {
                source: crate::cpu::x86::bits::Crc32Source::Memory { .. },
                ..
            }
            | Operation::MultiplyExtended {
                source: Value32::Memory(_),
                ..
            }
            | Operation::BitManipulation {
                source: Value32::Memory(_),
                ..
            }
            | Operation::Control(
                crate::cpu::x86::ControlOperation::PopFlags { .. }
                    | crate::cpu::x86::ControlOperation::Bound { .. }
            )
            | Operation::BitCount {
                source: Value32::Memory(_),
                ..
            }
            | Operation::MoveByteSwap { .. }
            | Operation::StackFrame { .. }
            | Operation::AtomicMemory { .. }
            | Operation::CompareExchange8B { .. }
            | Operation::VirtualTimestamp
            | Operation::TranslateByteAddress { .. }
            | Operation::X87 { .. }
            | Operation::Simd(_)
            | Operation::String { .. }
            | Operation::LoadSegment { .. }
            | Operation::PopSegment { .. }
            | Operation::LoadFarPointer { .. }
            | Operation::RepeatString { .. }
            | Operation::ReadSegment { .. }
            | Operation::PushSegment { .. }
            | Operation::LoadByte { .. }
            | Operation::ReadExtendByteToWord { .. }
            | Operation::LoadWord { .. }
            | Operation::MemoryCompareWord { .. }
            | Operation::ReadCompareWord { .. }
            | Operation::ReadArithmeticWord { .. }
            | Operation::ReadLogicalWord { .. }
            | Operation::MemoryTestWord { .. }
            | Operation::TranslateByte
            | Operation::ReadConditionalMove { .. }
            | Operation::ReadConditionalMoveWord { .. }
            | Operation::ReadSignedMultiplyWord { .. }
            | Operation::ReadSignedMultiplyImmediateWord { .. }
            | Operation::ReadMultiplyAccumulator { .. }
            | Operation::ReadWordMultiplyAccumulator { .. }
            | Operation::ReadByteMultiplyAccumulator { .. }
            | Operation::ReadByteDivideAccumulator { .. }
            | Operation::ReadWordDivideAccumulator { .. }
            | Operation::ReadDivideAccumulator { .. }
            | Operation::ReadTrailingZeroCount32 { .. }
            | Operation::ReadBitScan { .. }
            | Operation::ReadBitScanWord { .. }
            | Operation::ReadBitTest { .. }
            | Operation::ReadBitTestWord { .. }
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
        } | Operation::JumpWord { .. }
            | Operation::ReadJumpWord { .. }
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

fn supports_narrow_store(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::X87StatusToMemory { .. }
            | Operation::X87ControlToMemory { .. }
            | Operation::StoreByte { .. }
            | Operation::StoreWord { .. }
            | Operation::MemorySetByte { .. }
    )
}

fn supports_memory_unary(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::MemoryUnaryByte { .. }
            | Operation::MemoryUnaryWord { .. }
            | Operation::Unary {
                kind: UnaryKind::Inc | UnaryKind::Dec | UnaryKind::Not | UnaryKind::Neg,
                destination: Location32::Memory(_),
            }
    )
}

fn supports_memory_binary(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::MemoryArithmeticWord { .. }
            | Operation::MemoryLogicalWord { .. }
            | Operation::MemoryLogicalByte { .. }
            | Operation::MemoryBitMutation { .. }
            | Operation::LockedMemoryBitSet32 { .. }
            | Operation::LockedMemoryBitReset32 { .. }
            | Operation::LockedMemoryOr32 { .. }
            | Operation::LockedMemoryAnd32 { .. }
            | Operation::LockedMemoryXor32 { .. }
            | Operation::LockedMemoryOrRegister32 { .. }
            | Operation::LockedMemoryAndRegister32 { .. }
            | Operation::LockedMemoryXorRegister32 { .. }
            | Operation::MemoryBitMutationWord { .. }
            | Operation::MemoryArithmeticByte { .. }
            | Operation::MemoryExchange { .. }
            | Operation::MemoryExchangeByte { .. }
            | Operation::MemoryExchangeWord { .. }
            | Operation::MemoryExchangeAdd { .. }
            | Operation::LockedMemoryExchangeAdd32 { .. }
            | Operation::MemoryExchangeAddWord { .. }
            | Operation::MemoryExchangeAddByte { .. }
            | Operation::LockedMemoryExchangeAddByte { .. }
            | Operation::LockedMemoryExchangeAddWord { .. }
            | Operation::MemoryCompareExchange { .. }
            | Operation::LockedMemoryCompareExchange32 { .. }
            | Operation::LockedMemoryCompareExchangeByte { .. }
            | Operation::LockedMemoryCompareExchangeWord { .. }
            | Operation::MemoryCompareExchangeWord { .. }
            | Operation::MemoryCompareExchangeByte { .. }
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

fn supports_memory_shift_or_rotate(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::MemoryShiftByte { .. }
            | Operation::MemoryShiftWord { .. }
            | Operation::MemoryShiftByteImmediate { .. }
            | Operation::MemoryShiftByteCl { .. }
            | Operation::MemoryDoubleShift { .. }
            | Operation::MemoryDoubleShiftWord { .. }
            | Operation::MemoryByteRotateOne { .. }
            | Operation::MemoryByteRotate { .. }
            | Operation::MemoryByteRotateThroughCarryOne { .. }
            | Operation::MemoryByteRotateThroughCarryImmediate { .. }
            | Operation::MemoryByteRotateThroughCarryCl { .. }
            | Operation::MemoryRotateOne { .. }
            | Operation::MemoryRotate { .. }
            | Operation::MemoryRotateWord { .. }
            | Operation::MemoryRotateThroughCarryWord { .. }
            | Operation::MemoryRotateThroughCarryOne { .. }
            | Operation::MemoryRotateThroughCarryImmediate { .. }
            | Operation::MemoryRotateThroughCarryCl { .. }
            | Operation::Shift {
                destination: Location32::Memory(_),
                ..
            }
    )
}

pub(super) fn supports(instruction: &DecodedInstruction, embedded: bool) -> bool {
    let operation = instruction.operation();
    match operation {
        Operation::FarJump { .. }
        | Operation::FarCall { .. }
        | Operation::FarReturn { .. }
        | Operation::InterruptReturn { .. }
        | Operation::XState(_)
        | Operation::Avx(_)
        | Operation::VerifySegment { .. }
        | Operation::QuerySegment { .. } => {
            return embedded;
        }
        Operation::AdjustPrivilege { destination, .. } => {
            return embedded || matches!(destination, Location32::Register(_));
        }
        Operation::CacheFlush { .. } | Operation::VirtualTimestampAux => {
            return embedded;
        }
        Operation::PackedStringCompare(operation) => {
            return embedded && operation.valid();
        }
        Operation::Crc32 { source, .. } => {
            return embedded
                || !matches!(source, crate::cpu::x86::bits::Crc32Source::Memory { .. });
        }
        Operation::BitManipulation { source, .. } | Operation::MultiplyExtended { source, .. } => {
            return embedded || !matches!(source, Value32::Memory(_));
        }
        Operation::Control(operation) => return embedded || !operation.reads_memory(),
        Operation::BitCount { source, .. } => {
            return embedded || !matches!(source, Value32::Memory(_));
        }
        Operation::MoveByteSwap { .. } => return embedded,
        _ => {}
    }
    (embedded
        && (supports_memory_reads(operation)
            || supports_memory_move_store(operation)
            || supports_narrow_store(operation)
            || supports_memory_unary(operation)
            || supports_memory_binary(operation)
            || supports_memory_shift_or_rotate(operation)
            || supports_stack(instruction)
            || supports_indirect_jump(operation)))
        || matches!(
            operation,
            Operation::CpuId
                | Operation::ReadProcessorId { .. }
                | Operation::Nop
                | Operation::InitializeX87
                | Operation::ClearX87Exceptions
                | Operation::X87StatusToAx
                | Operation::Carry { .. }
                | Operation::DecimalAdjust { .. }
                | Operation::AsciiAdjust { .. }
                | Operation::AsciiAdjustBeforeDivision { .. }
                | Operation::AsciiAdjustAfterMultiplication { .. }
                | Operation::Direction { .. }
                | Operation::FlagsToAh
                | Operation::AhToFlags
                | Operation::MoveByte { .. }
                | Operation::MoveWord { .. }
                | Operation::ArithmeticWord { .. }
                | Operation::LogicalWord { .. }
                | Operation::CompareWord { .. }
                | Operation::TestWord { .. }
                | Operation::CompareByte { .. }
                | Operation::TestByte { .. }
                | Operation::LogicalByte { .. }
                | Operation::ArithmeticByte { .. }
                | Operation::UnaryByte { .. }
                | Operation::UnaryWord { .. }
                | Operation::ShiftWord { .. }
                | Operation::RotateWord { .. }
                | Operation::RotateThroughCarryWord { .. }
                | Operation::ShiftByte { .. }
                | Operation::DoubleShift { .. }
                | Operation::DoubleShiftWord { .. }
                | Operation::ByteRotateImmediate { .. }
                | Operation::ByteRotateCl { .. }
                | Operation::ByteRotateOne { .. }
                | Operation::RotateOne { .. }
                | Operation::Rotate { .. }
                | Operation::RotateThroughCarryOne { .. }
                | Operation::RotateThroughCarryImmediate { .. }
                | Operation::RotateThroughCarryCl { .. }
                | Operation::ByteRotateThroughCarryOne { .. }
                | Operation::ByteRotateThroughCarryImmediate { .. }
                | Operation::ByteRotateThroughCarryCl { .. }
                | Operation::SetByte { .. }
                | Operation::ConditionalMove { .. }
                | Operation::ConditionalMoveWord { .. }
                | Operation::SignExtendAccumulatorWord
                | Operation::SignExtendHighWord
                | Operation::SignExtendHigh
                | Operation::SignedMultiplyWord { .. }
                | Operation::SignedMultiplyImmediateWord { .. }
                | Operation::MultiplyAccumulator { .. }
                | Operation::WordMultiplyAccumulator { .. }
                | Operation::ByteMultiplyAccumulator { .. }
                | Operation::DivideAccumulator { .. }
                | Operation::WordDivideAccumulator { .. }
                | Operation::ByteDivideAccumulator { .. }
                | Operation::TrailingZeroCount32 { .. }
                | Operation::BitScan { .. }
                | Operation::BitScanWord { .. }
                | Operation::BitTest { .. }
                | Operation::BitTestWord { .. }
                | Operation::ByteSwap { .. }
                | Operation::Exchange { .. }
                | Operation::ExchangeWord { .. }
                | Operation::ExchangeAdd { .. }
                | Operation::ExchangeAddWord { .. }
                | Operation::ExchangeAddByte { .. }
                | Operation::CompareExchange { .. }
                | Operation::CompareExchangeWord { .. }
                | Operation::CompareExchangeByte { .. }
                | Operation::ExchangeByte { .. }
                | Operation::Move {
                    destination: Location32::Register(_),
                    source: Value32::Register(_) | Value32::Immediate(_),
                }
                | Operation::Lea { .. }
                | Operation::LeaWord { .. }
                | Operation::ExtendByteToWord { .. }
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
                | Operation::CountBranch { .. }
                | Operation::CountBranchWord { .. }
        )
}

#[cfg(test)]
#[path = "region/avx/native.rs"]
mod avx;
