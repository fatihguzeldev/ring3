use crate::cpu::dbt::wasm::helpers::protocol;
use crate::{
    abi::{
        X86_INTEGER_PROFILE,
        arena::{SIMD_OFFSET, TRANSFER_OFFSET, YMM_OFFSET},
        x86::{
            GATHER_ACCESS_OFFSET, GATHER_ADDRESS_BASE_OFFSET, GATHER_ADDRESS_MAGIC,
            GATHER_ADDRESS_SEGMENT_OFFSET, GATHER_ADDRESS_SIZE, GATHER_ADDRESS_TRANSFER,
            GATHER_ADDRESS_VERSION, GATHER_COMPLETED_OFFSET, GATHER_DESTINATION_OFFSET,
            GATHER_DETAIL_OFFSET, GATHER_FAULT_ADDRESS_OFFSET, GATHER_FAULT_LANE_OFFSET,
            GATHER_MASK_OFFSET, GATHER_NO_FAULT, GATHER_OUTPUT_SIZE, GATHER_OUTPUT_TRANSFER,
            GATHER_RESULT_MAGIC, GATHER_RESULT_SIZE, GATHER_RESULT_VERSION, GATHER_STATUS_OFFSET,
            SIMD_XMM_OFFSET, YMM_REGISTERS_OFFSET,
        },
    },
    cpu::{
        dbt::wasm::{
            imports::Imports,
            instructions::{avx::locals::AvxLocals, segments},
            locals::*,
        },
        x86::{
            SegmentRegister,
            avx::{AvxOperation, AvxSemantic, GatherAddress, GatherInstruction, VectorRegister},
        },
    },
};
use wasm_encoder::{BlockType, InstructionSink, MemArg, ValType};
mod paging;

pub(super) fn instruction(
    code: &mut InstructionSink<'_>,
    operation: AvxOperation,
    imports: Imports,
    locals: AvxLocals,
    exit_depth: u32,
) {
    let value = operation.instruction();
    let AvxSemantic::Gather(gather) = value.semantic() else {
        unreachable!("gather lowering requires gather semantics")
    };
    let address = operation.gather_address().expect("gather address");
    capture(code, gather, locals);
    address_metadata(code, address, locals.saved);
    let execute = imports
        .avx_execute
        .expect("gather execution import")
        .emit_binding(code);
    let (descriptor, operands, scale) = value.encode();
    code.i32_const(descriptor as i32)
        .i32_const(operands as i32)
        .i32_const(scale as i32)
        .i32_const(0)
        .i32_const(0)
        .local_get(FLAGS)
        .call(execute)
        .local_set(HELPER_STATUS);
    crate::cpu::dbt::wasm::code_fetch::check_context(code, exit_depth);

    // host failures never authorize staged vector publication.
    if imports.paging_configured {
        code.local_get(HELPER_STATUS)
            .i32_eqz()
            .local_get(HELPER_STATUS)
            .i32_const(crate::abi::memory_helper::CPU_PAGE_FAULT_TRANSPORT as i32)
            .i32_eq()
            .i32_or()
            .i32_eqz();
    } else {
        code.local_get(HELPER_STATUS);
    }
    code.if_(BlockType::Empty);
    protocol::infrastructure(code, 3);
    code.end();
    protocol::exit_if_failed(code, exit_depth);
    validate_result(
        code,
        gather,
        address,
        locals,
        exit_depth,
        imports.paging_configured,
    );
    validate_outputs(code, gather, locals, exit_depth);
    // the ordinary avx paths reuse this local as their transfer payload pointer.
    code.local_get(STATE_PTR)
        .i32_const(TRANSFER_OFFSET as i32)
        .i32_add()
        .local_set(locals.operand);
    publish(code, gather.destination(), GATHER_DESTINATION_OFFSET);
    publish(code, gather.mask(), GATHER_MASK_OFFSET);
    operand_exit(
        code,
        address.resolved_segment(),
        gather.geometry().data_bytes(),
        exit_depth,
        imports.paging_configured,
    );
}

fn capture(code: &mut InstructionSink<'_>, gather: GatherInstruction, locals: AvxLocals) {
    for (input, register) in [gather.destination(), gather.mask(), gather.index()]
        .into_iter()
        .enumerate()
    {
        for upper in [false, true] {
            code.local_get(STATE_PTR)
                .v128_load(vector(register, upper))
                .local_set(locals.vectors[input * 2 + usize::from(upper)]);
        }
    }
    code.i32_const(0);
    let geometry = gather.geometry();
    for lane in 0..geometry.lanes() {
        let byte = (lane + 1) * geometry.data_bytes() - 1;
        code.local_get(locals.vectors[2 + byte / 16])
            .i8x16_extract_lane_u((byte % 16) as u8)
            .i32_const(0x80)
            .i32_and()
            .if_(BlockType::Result(ValType::I32))
            .i32_const(1 << lane)
            .else_()
            .i32_const(0)
            .end()
            .i32_or();
    }
    code.local_set(locals.saved);
}

fn address_metadata(code: &mut InstructionSink<'_>, address: GatherAddress, selected: u32) {
    for (offset, value) in [
        (0, u32::from_le_bytes(GATHER_ADDRESS_MAGIC)),
        (
            4,
            u32::from(GATHER_ADDRESS_VERSION) | u32::from(X86_INTEGER_PROFILE) << 16,
        ),
        (8, GATHER_ADDRESS_SIZE as u32),
        (12, 0),
        (
            GATHER_ADDRESS_SEGMENT_OFFSET,
            address.resolved_segment().index() as u32,
        ),
        (GATHER_ADDRESS_BASE_OFFSET, 0),
    ] {
        code.local_get(STATE_PTR)
            .i32_const(value as i32)
            .i32_store(arg(TRANSFER_OFFSET + GATHER_ADDRESS_TRANSFER + offset));
    }
    code.i32_const(0).local_set(ADDRESS);
    // no scalar-base authority or arithmetic is consumed for an empty selection.
    code.local_get(selected)
        .if_(BlockType::Empty)
        .local_get(STATE_PTR)
        .i32_const(address.displacement() as i32);
    if let Some(base) = address.base() {
        code.local_get(register(base)).i32_add();
    }
    code.local_tee(ADDRESS)
        .i32_store(arg(TRANSFER_OFFSET
            + GATHER_ADDRESS_TRANSFER
            + GATHER_ADDRESS_BASE_OFFSET))
        .end();
}

fn validate_result(
    code: &mut InstructionSink<'_>,
    gather: GatherInstruction,
    address: GatherAddress,
    locals: AvxLocals,
    exit_depth: u32,
    paging_configured: bool,
) {
    protocol::reset_checks(code);
    for (offset, value) in [
        (0, u32::from_le_bytes(GATHER_RESULT_MAGIC)),
        (
            4,
            u32::from(GATHER_RESULT_VERSION) | u32::from(X86_INTEGER_PROFILE) << 16,
        ),
        (8, GATHER_RESULT_SIZE as u32),
        (12, 0),
    ] {
        if !paging_configured || offset != 4 {
            protocol::expect_field(code, offset as u64, value);
        }
    }
    field(code, GATHER_STATUS_OFFSET);
    code.local_set(RHS);
    field(code, GATHER_COMPLETED_OFFSET);
    code.local_set(locals.control);
    field(code, GATHER_FAULT_LANE_OFFSET);
    code.local_set(locals.operand);
    let segment_status = if address.resolved_segment() == SegmentRegister::Ss {
        7
    } else {
        3
    };
    if paging_configured {
        paging::validate_kind(code, segment_status);
    } else {
        code.local_get(RHS)
            .i32_eqz()
            .i32_eqz()
            .local_get(RHS)
            .i32_const(1)
            .i32_ne()
            .i32_and()
            .local_get(RHS)
            .i32_const(segment_status)
            .i32_ne()
            .i32_and();
        protocol::add_check(code);
    }
    code.local_get(RHS).i32_eqz().if_(BlockType::Empty);
    code.local_get(locals.control)
        .local_get(locals.saved)
        .i32_ne();
    protocol::add_check(code);
    expect_local(code, locals.operand, GATHER_NO_FAULT);
    for offset in [
        GATHER_DETAIL_OFFSET,
        GATHER_FAULT_ADDRESS_OFFSET,
        GATHER_ACCESS_OFFSET,
    ] {
        protocol::expect_field(code, offset as u64, 0);
    }
    code.else_();
    code.local_get(locals.saved).i32_eqz();
    protocol::add_check(code);
    code.local_get(locals.operand)
        .i32_const(gather.geometry().lanes() as i32)
        .i32_ge_u();
    protocol::add_check(code);
    code.local_get(locals.saved)
        .i32_const(1)
        .local_get(locals.operand)
        .i32_shl()
        .i32_and()
        .i32_eqz();
    protocol::add_check(code);
    // exact ascending selected-prefix: no skipped lower lane or completed higher lane.
    code.local_get(locals.control)
        .local_get(locals.saved)
        .i32_const(1)
        .local_get(locals.operand)
        .i32_shl()
        .i32_const(1)
        .i32_sub()
        .i32_and()
        .i32_ne();
    protocol::add_check(code);
    if paging_configured {
        paging::is_memory(code);
    } else {
        code.local_get(RHS).i32_const(1).i32_eq();
    }
    code.if_(BlockType::Empty);
    if paging_configured {
        paging::validate_memory_fields(code);
    } else {
        protocol::expect_field(code, GATHER_ACCESS_OFFSET as u64, 1);
    }
    for lane in 0..gather.geometry().lanes() {
        code.local_get(locals.operand)
            .i32_const(lane as i32)
            .i32_eq()
            .if_(BlockType::Empty);
        validate_fault_span(code, gather, address, locals, lane, paging_configured);
        code.end();
    }
    code.else_();
    for offset in [
        GATHER_DETAIL_OFFSET,
        GATHER_FAULT_ADDRESS_OFFSET,
        GATHER_ACCESS_OFFSET,
    ] {
        protocol::expect_field(code, offset as u64, 0);
    }
    code.end().end();
    protocol::protocol_if_bad(code);
    protocol::exit_if_failed(code, exit_depth);
}

fn validate_fault_span(
    code: &mut InstructionSink<'_>,
    gather: GatherInstruction,
    address: GatherAddress,
    locals: AvxLocals,
    lane: usize,
    paging_configured: bool,
) {
    let geometry = gather.geometry();
    let byte = lane * geometry.index_bytes();
    // only low32 index bits participate in protected32 modular address arithmetic.
    code.local_get(locals.vectors[4 + byte / 16])
        .i32x4_extract_lane((byte % 16 / 4) as u8)
        .i32_const(gather.scale().multiplier() as i32)
        .i32_mul()
        .local_get(ADDRESS)
        .i32_add();
    segments::base(code, address.resolved_segment());
    code.i32_add().local_set(RESULT);
    let width = geometry.data_bytes() as u32;
    if !paging_configured {
        field(code, GATHER_DETAIL_OFFSET);
        code.i32_const(1).i32_sub().i32_const(1).i32_gt_u();
        protocol::add_check(code);
    }
    field(code, GATHER_FAULT_ADDRESS_OFFSET);
    code.local_get(RESULT)
        .i32_sub()
        .i32_const(width as i32)
        .i32_ge_u();
    protocol::add_check(code);
}

fn validate_outputs(
    code: &mut InstructionSink<'_>,
    gather: GatherInstruction,
    locals: AvxLocals,
    exit_depth: u32,
) {
    protocol::reset_checks(code);
    let geometry = gather.geometry();
    for byte in (0..GATHER_OUTPUT_SIZE / 2).step_by(4) {
        if byte < geometry.active_bytes() {
            let bit = 1 << (byte / geometry.data_bytes());
            code.local_get(locals.control)
                .i32_const(bit)
                .i32_and()
                .i32_eqz()
                .if_(BlockType::Empty);
            output_word(code, GATHER_DESTINATION_OFFSET + byte);
            old_destination_word(code, locals, byte);
            code.i32_ne();
            protocol::add_check(code);
            code.end();
            output_word(code, GATHER_MASK_OFFSET + byte);
            code.local_get(locals.saved)
                .local_get(locals.control)
                .i32_const(-1)
                .i32_xor()
                .i32_and()
                .i32_const(bit)
                .i32_and()
                .if_(BlockType::Result(ValType::I32))
                .i32_const(-1)
                .else_()
                .i32_const(0)
                .end()
                .i32_ne();
            protocol::add_check(code);
        } else {
            output_word(code, GATHER_DESTINATION_OFFSET + byte);
            if geometry.early_zero_unused_destination() {
                code.i32_const(0);
            } else {
                code.local_get(RHS)
                    .i32_eqz()
                    .if_(BlockType::Result(ValType::I32))
                    .i32_const(0)
                    .else_();
                old_destination_word(code, locals, byte);
                code.end();
            }
            code.i32_ne();
            protocol::add_check(code);
            output_word(code, GATHER_MASK_OFFSET + byte);
            protocol::add_check(code);
        }
    }
    protocol::protocol_if_bad(code);
    protocol::exit_if_failed(code, exit_depth);
}

fn operand_exit(
    code: &mut InstructionSink<'_>,
    segment: SegmentRegister,
    width: usize,
    exit_depth: u32,
    paging_configured: bool,
) {
    if paging_configured {
        paging::operand_exit(code, segment, width);
    } else {
        code.local_get(RHS).if_(BlockType::Empty);
        code.local_get(RHS)
            .i32_const(1)
            .i32_eq()
            .if_(BlockType::Empty);
        code.i32_const(5).local_set(REASON);
        field(code, GATHER_DETAIL_OFFSET);
        code.local_set(DETAIL);
        field(code, GATHER_FAULT_ADDRESS_OFFSET);
        code.local_set(FAULT_ADDRESS);
        code.i32_const(1)
            .local_set(FAULT_ACCESS)
            .i32_const(width as i32)
            .local_set(FAULT_LENGTH);
        code.else_()
            .i32_const(if segment == SegmentRegister::Ss {
                15
            } else {
                11
            })
            .local_set(REASON);
        for local in [DETAIL, FAULT_ADDRESS, FAULT_ACCESS, FAULT_LENGTH] {
            code.i32_const(0).local_set(local);
        }
        code.end().end();
    }
    protocol::exit_if_failed(code, exit_depth);
}

fn publish(code: &mut InstructionSink<'_>, register: VectorRegister, offset: usize) {
    for upper in [false, true] {
        code.local_get(STATE_PTR)
            .local_get(STATE_PTR)
            .v128_load(arg(TRANSFER_OFFSET
                + GATHER_OUTPUT_TRANSFER
                + offset
                + usize::from(upper) * 16))
            .v128_store(vector(register, upper));
    }
}
fn old_destination_word(code: &mut InstructionSink<'_>, locals: AvxLocals, byte: usize) {
    code.local_get(locals.vectors[byte / 16])
        .i32x4_extract_lane((byte % 16 / 4) as u8);
}
fn expect_local(code: &mut InstructionSink<'_>, local: u32, expected: u32) {
    code.local_get(local).i32_const(expected as i32).i32_ne();
    protocol::add_check(code);
}
fn field(code: &mut InstructionSink<'_>, offset: usize) {
    protocol::helper_field(code, offset as u64);
}
fn output_word(code: &mut InstructionSink<'_>, offset: usize) {
    code.local_get(STATE_PTR)
        .i32_load(arg(TRANSFER_OFFSET + GATHER_OUTPUT_TRANSFER + offset));
}
fn vector(register: VectorRegister, upper: bool) -> MemArg {
    arg(if upper {
        YMM_OFFSET + YMM_REGISTERS_OFFSET
    } else {
        SIMD_OFFSET + SIMD_XMM_OFFSET
    } + register.index() * 16)
}
fn arg(offset: usize) -> MemArg {
    MemArg {
        offset: offset as u64,
        align: 0,
        memory_index: 0,
    }
}
