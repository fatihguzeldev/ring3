use super::{EmbeddedBinding, helpers::protocol, locals::HELPER_STATUS};
use crate::cpu::{
    dbt::{CompilationTier32, CompileError, InstructionError, PreparedRegion},
    x86::{X87Operation, ir::Operation},
};
use wasm_encoder::InstructionSink;

pub(in crate::cpu::dbt) fn emit(
    prepared: &PreparedRegion,
    tier: CompilationTier32,
    key: u64,
    id: u64,
) -> Result<Vec<u8>, CompileError> {
    if prepared.blocks.is_empty() {
        return Err(CompileError::InvalidBlocks);
    }
    for block in &prepared.blocks {
        if block.gate().is_some()
            || block.context_fault().is_some()
            || block.instructions.is_empty()
        {
            return Err(CompileError::InvalidBlocks);
        }
        for instruction in &block.instructions {
            if !supported(instruction.operation()) {
                return Err(CompileError::Instruction {
                    pc: instruction.pc(),
                    cause: InstructionError::BackendUnsupported,
                });
            }
        }
    }
    Ok(super::emitter::emit_profile(
        &prepared.blocks,
        Some(EmbeddedBinding::Resident { key, id }),
        prepared.decode_key,
        tier,
        true,
    ))
}

fn supported(operation: &Operation) -> bool {
    match operation {
        Operation::Nop
        | Operation::Move { .. }
        | Operation::Binary { .. }
        | Operation::Unary { .. }
        | Operation::Carry { .. }
        | Operation::Direction { .. }
        | Operation::Lea { .. }
        | Operation::Jump { .. }
        | Operation::ConditionalJump { .. }
        | Operation::CountBranch { .. }
        | Operation::Push { .. }
        | Operation::Pop { .. }
        | Operation::Call { .. }
        | Operation::Return { .. }
        | Operation::X87StatusToAx
        | Operation::X87StatusToMemory { .. }
        | Operation::X87ControlToMemory { .. } => true,
        Operation::Control(crate::cpu::x86::ControlOperation::InvalidOpcode) => true,
        Operation::X87 { operation, .. } => matches!(
            operation,
            X87Operation::Wait
                | X87Operation::Load(_)
                | X87Operation::Store { .. }
                | X87Operation::LoadRegister { .. }
                | X87Operation::StoreRegister { .. }
                | X87Operation::StoreRegisterNoUnderflow { .. }
                | X87Operation::Exchange { .. }
                | X87Operation::Binary { .. }
                | X87Operation::BinaryMemory { .. }
                | X87Operation::Compare { .. }
                | X87Operation::CompareMemory { .. }
                | X87Operation::ConditionalMove { .. }
                | X87Operation::Test
                | X87Operation::Examine
                | X87Operation::ChangeSign
                | X87Operation::Absolute
                | X87Operation::SquareRoot
                | X87Operation::RoundToInteger
                | X87Operation::Scale
                | X87Operation::Extract
                | X87Operation::Remainder { .. }
                | X87Operation::LoadConstant { .. }
                | X87Operation::Free { .. }
                | X87Operation::AdjustTop { .. }
                | X87Operation::LoadControl
                | X87Operation::StoreControl
                | X87Operation::StoreStatus
                | X87Operation::StatusToAx
                | X87Operation::NoOperation
                | X87Operation::Exp2MinusOne
                | X87Operation::Log2Multiply { .. }
                | X87Operation::PartialAtan
                | X87Operation::Trigonometric { .. }
        ),
        _ => false,
    }
}

// transport zero can still carry a guest fault; both success conditions are required.
pub(super) fn success(code: &mut InstructionSink<'_>) {
    code.local_get(HELPER_STATUS).i32_eqz();
    protocol::helper_field(code, 16);
    code.i32_eqz().i32_and();
}

pub(super) fn full_group(
    code: &mut InstructionSink<'_>,
    limit: u32,
    completed: u32,
    reported: u32,
    length: u32,
) {
    success(code);
    for (local, expected) in [(limit, length), (completed, length), (reported, length - 1)] {
        code.local_get(local)
            .i32_const(expected as i32)
            .i32_eq()
            .i32_and();
    }
}

pub(super) fn zero_scratch(code: &mut InstructionSink<'_>) {
    code.i32_const(0)
        .local_set(super::locals::LHS)
        .i32_const(0)
        .local_set(super::locals::RHS);
}

pub(super) fn flags(code: &mut InstructionSink<'_>) {
    protocol::helper_field(code, 20);
    code.local_tee(super::locals::RESULT)
        .local_set(super::locals::FLAGS);
    zero_scratch(code);
}
