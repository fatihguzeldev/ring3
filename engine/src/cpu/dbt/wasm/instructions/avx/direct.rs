use crate::{
    abi::{
        arena::{SIMD_OFFSET, YMM_OFFSET},
        x86::{REGISTERS_OFFSET, SIMD_MXCSR_OFFSET, SIMD_XMM_OFFSET, YMM_REGISTERS_OFFSET},
    },
    cpu::{
        dbt::wasm::{
            helpers::result as cpu_result,
            imports::Imports,
            instructions::{
                avx::{VectorLayout, locals::AvxLocals},
                memory,
            },
            locals::*,
        },
        x86::{
            SegmentAccess,
            avx::{AvxInstruction, AvxOperation, AvxSemantic, MemoryEffect, MoveInstruction},
        },
    },
    memory::Access,
};
use wasm_encoder::{BlockType, InstructionSink, MemArg};
pub(super) fn accepts(instruction: AvxInstruction) -> bool {
    matches!(
        instruction.semantic(),
        AvxSemantic::Move(_)
            | AvxSemantic::Logical(_)
            | AvxSemantic::Permute(_)
            | AvxSemantic::Integer(_)
            | AvxSemantic::Rearrange(_)
            | AvxSemantic::Shift(_)
            | AvxSemantic::Multiply(_)
            | AvxSemantic::Scalar(_)
            | AvxSemantic::CrossLane(_)
    )
}

pub(super) fn instruction(
    code: &mut InstructionSink<'_>,
    operation: AvxOperation,
    imports: Imports,
    locals: AvxLocals,
    exit_depth: u32,
) -> bool {
    let instruction = operation.instruction();
    assert!(accepts(instruction));
    let requirement = match instruction.memory() {
        MemoryEffect::None => None,
        MemoryEffect::Full(requirement) => Some(requirement),
        MemoryEffect::Selected(_) | MemoryEffect::Gather(_) => {
            unreachable!("specialized accesses belong to the avx helper")
        }
    };
    if let Some(requirement) = requirement {
        memory::checked_address(
            code,
            operation.address().expect("checked avx memory address"),
            u32::from(requirement.bytes),
            if requirement.access == Access::Read {
                SegmentAccess::Read
            } else {
                SegmentAccess::Write
            },
            imports,
            exit_depth,
        );
        if requirement.alignment == 32 {
            code.local_get(ADDRESS).i32_const(31).i32_and();
            general_protection_if(code, exit_depth);
        }
        if requirement.access == Access::Read {
            code.local_get(ADDRESS)
                .i32_const(i32::from(requirement.bytes))
                .i32_const(i32::from(transport_alignment(requirement.alignment)))
                .call(imports.cpu_read.expect("avx read import"))
                .local_set(HELPER_STATUS);
            crate::cpu::dbt::wasm::code_fetch::check_context(code, exit_depth);
            cpu_result::validate(
                code,
                u32::from(requirement.bytes),
                1,
                false,
                exit_depth,
                cpu_result::ResultContext::from(imports),
            );
        }
    }
    if instruction.semantic() == AvxSemantic::Move(MoveInstruction::LoadControl) {
        code.local_get(locals.operand)
            .i32_load(arg(0))
            .i32_const(!0xffff)
            .i32_and();
        general_protection_if(code, exit_depth);
    }
    // only the explicit source registers need synchronization with their live locals.
    for input in instruction.inputs().into_iter().flatten() {
        code.local_get(STATE_PTR)
            .local_get(register(input))
            .i32_store(arg(REGISTERS_OFFSET + input.index() * 4));
    }
    let layout = VectorLayout {
        lower: (SIMD_OFFSET + SIMD_XMM_OFFSET) as u32,
        upper: (YMM_OFFSET + YMM_REGISTERS_OFFSET) as u32,
    };
    let vectors = locals.vectors;
    match instruction.semantic() {
        AvxSemantic::Move(value) => super::moves::emit(
            code,
            value,
            super::moves::MoveLayout {
                vectors: layout,
                general: REGISTERS_OFFSET as u32,
                mxcsr: (SIMD_OFFSET + SIMD_MXCSR_OFFSET) as u32,
            },
            super::moves::MoveLocals {
                state: STATE_PTR,
                operand: locals.operand,
                result: [vectors[0], vectors[1]],
                control: locals.control,
            },
            exit_depth,
        ),
        AvxSemantic::Logical(value) => super::logical::emit(
            code,
            value,
            layout,
            STATE_PTR,
            locals.operand,
            [vectors[0], vectors[1]],
        ),
        AvxSemantic::Permute(value) => super::permute::emit(
            code,
            value,
            super::permute::PermuteLayout {
                vectors: layout,
                general: REGISTERS_OFFSET as u32,
            },
            super::permute::PermuteLocals {
                state: STATE_PTR,
                operand: locals.operand,
                result: [vectors[0], vectors[1]],
            },
        ),
        AvxSemantic::Integer(value) => super::integer::emit(
            code,
            value,
            layout,
            super::integer::IntegerLocals {
                state: STATE_PTR,
                operand: locals.operand,
                scratch: [vectors[0], vectors[1]],
            },
        ),
        AvxSemantic::Rearrange(value) => super::rearrange::emit(
            code,
            value,
            layout,
            super::rearrange::RearrangeLocals {
                state: STATE_PTR,
                operand: locals.operand,
                sources: [vectors[0], vectors[1], vectors[2]],
            },
        ),
        AvxSemantic::Shift(value) => super::shift::emit(
            code,
            value,
            layout,
            super::shift::ShiftLocals {
                state: STATE_PTR,
                operand: locals.operand,
                sources: [vectors[0], vectors[1]],
            },
        ),
        AvxSemantic::Multiply(value) => super::multiply::emit(
            code,
            value,
            layout,
            super::multiply::MultiplyLocals {
                state: STATE_PTR,
                operand: locals.operand,
                sources: [vectors[0], vectors[1]],
                scratch: [RESULT, RHS, LHS],
            },
        ),
        AvxSemantic::Scalar(value) => super::scalar::emit(
            code,
            value,
            super::scalar::ScalarLayout {
                vectors: layout,
                general: REGISTERS_OFFSET as u32,
            },
            super::scalar::ScalarLocals {
                state: STATE_PTR,
                operand: locals.operand,
                vector: vectors[0],
                scalar: locals.control,
            },
        ),
        AvxSemantic::CrossLane(value) => super::cross_lane::emit(
            code,
            value,
            layout,
            super::cross_lane::CrossLaneLocals {
                state: STATE_PTR,
                operand: locals.operand,
                result: [vectors[0], vectors[1]],
            },
        ),
        _ => unreachable!("only accepted direct families reach this lowerer"),
    }
    if let Some(output) = instruction.output() {
        code.local_get(STATE_PTR)
            .i32_load(arg(REGISTERS_OFFSET + output.index() * 4))
            .local_set(register(output));
    }
    let Some(requirement) = requirement.filter(|value| value.access == Access::Write) else {
        return false;
    };
    let write = imports
        .cpu_write
        .expect("avx write import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .i32_const(i32::from(requirement.bytes))
        .i32_const(i32::from(transport_alignment(requirement.alignment)))
        .call(write)
        .local_set(HELPER_STATUS);
    cpu_result::validate(
        code,
        u32::from(requirement.bytes),
        2,
        true,
        exit_depth,
        cpu_result::ResultContext::from(imports),
    );
    true
}

fn transport_alignment(alignment: u8) -> u8 {
    // alignment32 is checked before the legacy span helper, whose contract remains unchanged.
    if alignment == 32 { 1 } else { alignment }
}

fn general_protection_if(code: &mut InstructionSink<'_>, exit_depth: u32) {
    code.if_(BlockType::Empty).i32_const(11).local_set(REASON);
    for local in [DETAIL, FAULT_ADDRESS, FAULT_ACCESS, FAULT_LENGTH] {
        code.i32_const(0).local_set(local);
    }
    code.br(exit_depth + 1).end();
}

fn arg(offset: usize) -> MemArg {
    MemArg {
        offset: offset as u64,
        align: 0,
        memory_index: 0,
    }
}
