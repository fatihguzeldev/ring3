use wasm_encoder::{BlockType, InstructionSink, MemArg, ValType};

use super::super::{
    cpu_result::{self, AvxValidation},
    locals::*,
    memory::{self, Imports},
    segments,
};
use super::locals::AvxLocals;
use crate::{
    abi::{
        X86_INTEGER_PROFILE,
        arena::{SIMD_OFFSET, TRANSFER_OFFSET, YMM_OFFSET},
        x86::{
            AVX_ADDRESS_LOGICAL_OFFSET, AVX_ADDRESS_MAGIC, AVX_ADDRESS_SEGMENT_OFFSET,
            AVX_ADDRESS_SIZE, AVX_ADDRESS_TRANSFER, AVX_ADDRESS_VERSION, AVX_OUTPUT_SIZE,
            AVX_OUTPUT_TRANSFER, SIMD_MXCSR_OFFSET, SIMD_XMM_OFFSET, YMM_REGISTERS_OFFSET,
        },
    },
    cpu::x86::{
        SegmentRegister,
        avx::{AvxOperation, AvxSemantic, MaskedInstruction, MemoryEffect, MoveInstruction},
        ir::EffectiveAddress,
    },
    memory::Access,
};

pub(super) fn instruction(
    code: &mut InstructionSink<'_>,
    operation: AvxOperation,
    imports: Imports,
    locals: AvxLocals,
    exit_depth: u32,
) -> bool {
    let instruction = operation.instruction();
    let (length, access, selected) = match instruction.memory() {
        MemoryEffect::Gather(_) => unreachable!("gather has a dedicated staged helper route"),
        MemoryEffect::None => (0, 0, None),
        MemoryEffect::Full(requirement) => (
            u32::from(requirement.bytes),
            access_mask(requirement.access),
            None,
        ),
        MemoryEffect::Selected(value) => {
            selected_bytes(code, value, locals);
            (
                value.width().bytes() as u32,
                access_mask(value.access()),
                Some(locals.saved),
            )
        }
    };
    if let Some(address) = operation.address() {
        address_metadata(code, address, selected);
    }
    code.local_get(STATE_PTR)
        .i32_load(arg(SIMD_OFFSET + SIMD_MXCSR_OFFSET))
        .local_set(locals.control);
    let execute = imports
        .avx_execute
        .expect("avx execution import")
        .emit_binding(code);
    let (descriptor, operands, immediate) = instruction.encode();
    code.i32_const(descriptor as i32)
        .i32_const(operands as i32)
        .i32_const(immediate as i32);
    for input in instruction.inputs() {
        if let Some(input) = input {
            code.local_get(register(input));
        } else {
            code.i32_const(0);
        }
    }
    code.local_get(FLAGS).call(execute).local_set(HELPER_STATUS);
    if matches!(instruction.semantic(), AvxSemantic::Test(_))
        || matches!(instruction.semantic(), AvxSemantic::String(value)
            if value.output() == crate::cpu::x86::packed_strings::PackedStringOutput::Index)
    {
        super::super::code_fetch::check_context(code, exit_depth);
    }
    let floating = matches!(
        instruction.semantic(),
        AvxSemantic::Floating(_)
            | AvxSemantic::Compare(_)
            | AvxSemantic::Convert(_)
            | AvxSemantic::Compound(_)
            | AvxSemantic::F16c(_)
            | AvxSemantic::Fma(_)
    );
    cpu_result::validate_avx(
        code,
        length,
        access,
        access == 2,
        AvxValidation {
            selected_mask: selected,
            old_mxcsr: floating.then_some(locals.control),
            general_protection: operation.address().is_some_and(|address| {
                address.resolved_segment() != SegmentRegister::Ss
                    || matches!(instruction.memory(), MemoryEffect::Full(value) if value.alignment > 1)
                    || matches!(instruction.semantic(), AvxSemantic::Move(MoveInstruction::LoadControl))
            }),
            stack_segment: operation.address().is_some_and(|address| {
                address.resolved_segment() == SegmentRegister::Ss
            }),
        },
        exit_depth,
    );
    memory::reset_checks(code);
    memory::expect_field(code, 20, 0);
    for offset in (8..AVX_OUTPUT_SIZE).step_by(4) {
        payload(code, offset);
        memory::add_check(code);
    }
    if instruction.output().is_none() {
        payload(code, 0);
        memory::add_check(code);
    }
    flags_shape(code, instruction.semantic());
    memory::protocol_if_bad(code);
    memory::exit_if_failed(code, exit_depth);
    payload(code, 4);
    cpu_result::flags(
        code,
        if instruction.writes_flags() { 0x8d5 } else { 0 },
        exit_depth,
    );
    if let Some(output) = instruction.output() {
        payload(code, 0);
        code.local_set(register(output));
    }
    access == 2
}

fn address_metadata(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    selected: Option<u32>,
) {
    for (offset, value) in [
        (0, u32::from_le_bytes(AVX_ADDRESS_MAGIC)),
        (
            4,
            u32::from(AVX_ADDRESS_VERSION) | (u32::from(X86_INTEGER_PROFILE) << 16),
        ),
        (8, AVX_ADDRESS_SIZE as u32),
        (12, 0),
        (
            AVX_ADDRESS_SEGMENT_OFFSET,
            address.resolved_segment().index() as u32,
        ),
        (AVX_ADDRESS_LOGICAL_OFFSET, 0),
    ] {
        code.local_get(STATE_PTR)
            .i32_const(value as i32)
            .i32_store(arg(TRANSFER_OFFSET + AVX_ADDRESS_TRANSFER + offset));
    }
    code.i32_const(0).local_set(ADDRESS);
    if let Some(mask) = selected {
        code.local_get(mask).if_(BlockType::Empty);
    }
    code.local_get(STATE_PTR);
    memory::offset_value(code, address);
    code.local_tee(ADDRESS).i32_store(arg(TRANSFER_OFFSET
        + AVX_ADDRESS_TRANSFER
        + AVX_ADDRESS_LOGICAL_OFFSET));
    code.local_get(ADDRESS);
    segments::base(code, address.resolved_segment());
    code.i32_add().local_set(ADDRESS);
    if selected.is_some() {
        code.end();
    }
}

fn selected_bytes(
    code: &mut InstructionSink<'_>,
    instruction: MaskedInstruction,
    locals: AvxLocals,
) {
    let lane = instruction.lane_bytes();
    code.i32_const(0);
    for half in 0..instruction.width().bytes() / 16 {
        let base = if half == 0 {
            SIMD_OFFSET + SIMD_XMM_OFFSET
        } else {
            YMM_OFFSET + YMM_REGISTERS_OFFSET
        };
        code.local_get(STATE_PTR)
            .v128_load(arg(base + instruction.mask().index() * 16))
            .local_set(locals.vectors[0]);
        for start in (0..16).step_by(lane) {
            let mask = ((1u32 << lane) - 1) << (half * 16 + start);
            code.local_get(locals.vectors[0])
                .i8x16_extract_lane_u((start + lane - 1) as u8)
                .i32_const(0x80)
                .i32_and()
                .if_(BlockType::Result(ValType::I32))
                .i32_const(mask as i32)
                .else_()
                .i32_const(0)
                .end()
                .i32_or();
        }
    }
    code.local_set(locals.saved);
}

fn flags_shape(code: &mut InstructionSink<'_>, semantic: AvxSemantic) {
    match semantic {
        AvxSemantic::Compare(crate::cpu::x86::avx::CompareInstruction::Flags { .. }) => {
            payload(code, 4);
            code.i32_const(0x8d5).i32_and().local_set(RESULT);
            code.local_get(RESULT).i32_eqz().i32_eqz();
            for relation in [1, 0x40, 0x45] {
                code.local_get(RESULT)
                    .i32_const(relation)
                    .i32_ne()
                    .i32_and();
            }
            memory::add_check(code);
        }
        AvxSemantic::Test(_) => {
            payload(code, 4);
            code.i32_const(0x894).i32_and();
            memory::add_check(code);
        }
        AvxSemantic::String(_) => {
            payload(code, 4);
            code.i32_const(0x14).i32_and();
            memory::add_check(code);
            payload(code, 4);
            code.i32_const(0x800).i32_and().i32_eqz().i32_eqz();
            payload(code, 4);
            code.i32_const(1).i32_and().i32_eqz().i32_and();
            memory::add_check(code);
        }
        _ => {}
    }
}

fn access_mask(access: Access) -> u32 {
    match access {
        Access::Read => 1,
        Access::Write => 2,
        Access::Execute => unreachable!("avx operands are data"),
    }
}
fn payload(code: &mut InstructionSink<'_>, offset: usize) {
    code.local_get(STATE_PTR)
        .i32_load(arg(TRANSFER_OFFSET + AVX_OUTPUT_TRANSFER + offset));
}
fn arg(offset: usize) -> MemArg {
    MemArg {
        offset: offset as u64,
        align: 0,
        memory_index: 0,
    }
}
