use wasm_encoder::{BlockType, InstructionSink, MemArg};

use crate::{
    abi::{
        ABI_VERSION, X86_INTEGER_PROFILE,
        arena::X87_OFFSET,
        x86::{
            EXIT_SIZE, STATE_SIZE, X87_CODE_SELECTOR_OFFSET, X87_CONTROL_OFFSET,
            X87_DATA_POINTER_OFFSET, X87_INSTRUCTION_POINTER_OFFSET, X87_OPCODE_OFFSET, X87_SIZE,
            X87_STATUS_OFFSET, X87_TAG_OFFSET,
        },
    },
    cpu::x86::{Register32, X87State, ir::EffectiveAddress},
};

use super::locals::{
    ADDRESS, CANCEL_PTR, EXIT_PTR, FLAGS, HELPER_STATUS, MEMORY_BYTES, RESULT, STATE_PTR, register,
};

#[allow(clippy::too_many_arguments)]
pub(super) fn operation(
    code: &mut InstructionSink<'_>,
    operation: crate::cpu::x86::X87Operation,
    address: Option<crate::cpu::x86::ir::EffectiveAddress>,
    waiting: bool,
    pc: u32,
    opcode: u16,
    import: super::memory::StoreImport,
    exit_depth: u32,
) {
    if let Some(address) = address {
        super::memory::address_value(code, address);
    } else {
        code.i32_const(0);
    }
    code.local_set(ADDRESS);
    let index = import.emit_binding(code);
    code.i32_const(operation.encode() as i32)
        .local_get(ADDRESS)
        .local_get(FLAGS)
        .i32_const(pc as i32)
        .i32_const(i32::from(opcode))
        .i32_const(control_word(address, waiting) as i32)
        .call(index)
        .local_set(HELPER_STATUS);
    let length = operation.read_size().max(operation.write_size()) as u32;
    let access =
        u32::from(operation.read_size() != 0) | (u32::from(operation.write_size() != 0) << 1);
    super::cpu_result::validate(
        code,
        length,
        access,
        operation.write_size() != 0,
        exit_depth,
    );
    super::cpu_result::value(code);
    let integer_flags = matches!(
        operation,
        crate::cpu::x86::X87Operation::Compare {
            integer_flags: true,
            ..
        }
    );
    if integer_flags {
        code.local_set(RESULT);
        super::memory::reset_checks(code);
        // a deferred unmasked comparison preserves flags; a completed one has one of four relations.
        code.local_get(RESULT).local_get(FLAGS).i32_ne();
        for relation in [0, 1, 0x40, 0x45] {
            code.local_get(RESULT)
                .i32_const(0x8d5)
                .i32_and()
                .i32_const(relation)
                .i32_ne()
                .i32_and();
        }
        super::memory::add_check(code);
        super::memory::protocol_if_bad(code);
        super::memory::exit_if_failed(code, exit_depth);
        code.local_get(RESULT);
    }
    super::cpu_result::flags(code, if integer_flags { 0x8d5 } else { 0 }, exit_depth);
    if operation == crate::cpu::x86::X87Operation::StatusToAx {
        status_to_ax(code);
    }
}

fn control_word(address: Option<EffectiveAddress>, waiting: bool) -> u32 {
    let segment = address.map_or(0, |address| {
        let segment = address.resolved_segment();
        (segment.index() as u32 + 1) << 8
    });
    segment | u32::from(waiting)
}

pub(super) fn pending(code: &mut InstructionSink<'_>, exit_depth: u32) {
    use super::locals::{DETAIL, FAULT_ACCESS, FAULT_ADDRESS, FAULT_LENGTH, LHS, REASON};
    status_value(code);
    code.local_set(LHS);
    control_value(code);
    code.i32_const(-1)
        .i32_xor()
        .local_get(LHS)
        .i32_and()
        .i32_const(0x3f)
        .i32_and()
        .if_(BlockType::Empty);
    code.local_get(STATE_PTR)
        .local_get(LHS)
        .i32_const(0x8080)
        .i32_or()
        .local_tee(DETAIL)
        .i32_store16(memarg(X87_STATUS_OFFSET, 1));
    code.i32_const(12).local_set(REASON);
    for local in [FAULT_ADDRESS, FAULT_ACCESS, FAULT_LENGTH] {
        code.i32_const(0).local_set(local);
    }
    code.br(exit_depth + 1).end();
}

pub(super) fn preflight(code: &mut InstructionSink<'_>) {
    end(code);
    code.local_get(MEMORY_BYTES).i64_gt_u();
    fail_if(code, 1);
    for (pointer, size) in [
        (STATE_PTR, STATE_SIZE),
        (EXIT_PTR, EXIT_SIZE),
        (CANCEL_PTR, 4),
    ] {
        start(code);
        code.local_get(pointer)
            .i64_extend_i32_u()
            .i64_const(size as i64)
            .i64_add()
            .i64_lt_u()
            .local_get(pointer)
            .i64_extend_i32_u();
        end(code);
        code.i64_lt_u().i32_and();
        fail_if(code, 1);
    }
    for (offset, expected) in [
        (0, u32::from_le_bytes(*b"R3FP")),
        (
            4,
            u32::from(ABI_VERSION) | (u32::from(X86_INTEGER_PROFILE) << 16),
        ),
        (8, X87_SIZE as u32),
        (12, 0),
        (36, 0),
        (120, 0),
        (124, 0),
    ] {
        code.local_get(STATE_PTR)
            .i32_load(memarg(offset, 2))
            .i32_const(expected as i32)
            .i32_ne();
        fail_if(code, 2);
    }
    code.local_get(STATE_PTR)
        .i32_load16_u(memarg(X87_OPCODE_OFFSET, 1))
        .i32_const(0x7ff)
        .i32_gt_u();
    fail_if(code, 2);
}

pub(super) fn initialize(code: &mut InstructionSink<'_>) {
    let state = X87State::default();
    for (offset, value) in [
        (
            X87_CONTROL_OFFSET,
            u32::from(state.control) | (u32::from(state.status) << 16),
        ),
        (
            X87_TAG_OFFSET,
            u32::from(state.tag) | (u32::from(state.opcode) << 16),
        ),
        (X87_INSTRUCTION_POINTER_OFFSET, state.instruction_pointer),
        (X87_DATA_POINTER_OFFSET, state.data_pointer),
        (
            X87_CODE_SELECTOR_OFFSET,
            u32::from(state.code_selector) | (u32::from(state.data_selector) << 16),
        ),
    ] {
        code.local_get(STATE_PTR)
            .i32_const(value as i32)
            .i32_store(memarg(offset, 2));
    }
}

pub(super) fn clear_exceptions(code: &mut InstructionSink<'_>) {
    code.local_get(STATE_PTR)
        .local_get(STATE_PTR)
        .i32_load16_u(memarg(X87_STATUS_OFFSET, 1))
        .i32_const(0x7f00)
        .i32_and()
        .i32_store16(memarg(X87_STATUS_OFFSET, 1));
}

pub(super) fn status_to_ax(code: &mut InstructionSink<'_>) {
    code.local_get(register(Register32::Eax))
        .i32_const(!0xffff)
        .i32_and()
        .local_get(STATE_PTR)
        .i32_load16_u(memarg(X87_STATUS_OFFSET, 1))
        .i32_or()
        .local_set(register(Register32::Eax));
}

pub(super) fn status_value(code: &mut InstructionSink<'_>) {
    code.local_get(STATE_PTR)
        .i32_load16_u(memarg(X87_STATUS_OFFSET, 1));
}

pub(super) fn control_value(code: &mut InstructionSink<'_>) {
    code.local_get(STATE_PTR)
        .i32_load16_u(memarg(X87_CONTROL_OFFSET, 1));
}

fn start(code: &mut InstructionSink<'_>) {
    code.local_get(STATE_PTR)
        .i64_extend_i32_u()
        .i64_const(X87_OFFSET as i64)
        .i64_add();
}

fn end(code: &mut InstructionSink<'_>) {
    code.local_get(STATE_PTR)
        .i64_extend_i32_u()
        .i64_const((X87_OFFSET + X87_SIZE) as i64)
        .i64_add();
}

fn fail_if(code: &mut InstructionSink<'_>, status: i32) {
    code.if_(BlockType::Empty).i32_const(status).return_().end();
}

fn memarg(offset: usize, align: u32) -> MemArg {
    MemArg {
        offset: (X87_OFFSET + offset) as u64,
        align,
        memory_index: 0,
    }
}

#[cfg(test)]
mod pointer_control_tests {
    use super::*;
    use crate::cpu::x86::SegmentRegister;

    #[test]
    fn memory_control_carries_selected_segment_without_changing_waiting_bit() {
        assert_eq!(control_word(None, false), 0);
        assert_eq!(control_word(None, true), 1);
        for (address, wanted) in [
            (EffectiveAddress::default(), 0x401),
            (
                EffectiveAddress {
                    base: Some(Register32::Ebp),
                    ..EffectiveAddress::default()
                },
                0x301,
            ),
            (
                EffectiveAddress {
                    base: Some(Register32::Esp),
                    ..EffectiveAddress::default()
                },
                0x301,
            ),
            (
                EffectiveAddress {
                    segment: Some(SegmentRegister::Fs),
                    ..EffectiveAddress::default()
                },
                0x501,
            ),
            (
                EffectiveAddress {
                    base: Some(Register32::Ebp),
                    segment: Some(SegmentRegister::Gs),
                    ..EffectiveAddress::default()
                },
                0x601,
            ),
        ] {
            assert_eq!(control_word(Some(address), true), wanted);
            assert_eq!(control_word(Some(address), false), wanted & !1);
        }
    }
}
