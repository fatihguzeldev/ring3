use wasm_encoder::{BlockType, InstructionSink, ValType};

use super::super::{locals::*, memory};
use crate::cpu::x86::{
    SegmentRegister,
    xstate::{
        access::MemorySpan,
        upper::access::{RESTORE_SPANS, SAVE_SPANS},
    },
};

/// the standard image has reserved holes and a 256-byte component extent with 128 active bytes.
/// packet validation follows actual selected accesses rather than treating all 832 bytes as memory.
pub(super) fn validate_context(
    code: &mut InstructionSink<'_>,
    save: bool,
    segment: SegmentRegister,
    exit_depth: u32,
) {
    memory::reset_checks(code);
    memory::expect_field(code, 20, 0);
    memory::helper_field(code, 16);
    code.i32_const(3).i32_gt_u();
    if segment == SegmentRegister::Ss {
        memory::helper_field(code, 16);
        code.i32_const(7).i32_ne().i32_and();
    }
    memory::add_check(code);
    memory::helper_field(code, 16);
    code.i32_const(1).i32_le_u().if_(BlockType::Empty);
    code.local_get(ADDRESS).i32_const(63).i32_and();
    memory::add_check(code);
    code.end();
    memory::helper_field(code, 16);
    code.i32_const(3).i32_eq();
    if segment == SegmentRegister::Ss {
        memory::helper_field(code, 16);
        code.i32_const(7).i32_eq().i32_or();
    }
    code.if_(BlockType::Empty);
    memory::expect_field(code, 24, 0);
    code.end();
    memory::helper_field(code, 16);
    code.i32_eqz().if_(BlockType::Empty);
    success(code, save);
    code.end();
    memory::helper_field(code, 16);
    code.i32_const(1).i32_eq().if_(BlockType::Empty);
    fault(code, save);
    code.end();
    memory::protocol_if_bad(code);
    memory::exit_if_failed(code, exit_depth);
}

fn selected(code: &mut InstructionSink<'_>, selection: u32) {
    if selection == 0 {
        code.i32_const(1);
    } else {
        code.local_get(register(crate::cpu::x86::Register32::Eax))
            .i32_const(selection as i32)
            .i32_and()
            .i32_eqz()
            .i32_eqz();
    }
}

fn fits(code: &mut InstructionSink<'_>, end: u32) {
    code.local_get(ADDRESS).i32_const(-(end as i32)).i32_le_u();
}

fn success(code: &mut InstructionSink<'_>, save: bool) {
    fits(code, if save { 520 } else { 536 });
    code.i32_eqz();
    memory::add_check(code);
    if save {
        for span in SAVE_SPANS {
            selected(code, span.selection);
            fits(code, span.offset + span.width);
            code.i32_eqz().i32_and();
            memory::add_check(code);
        }
    } else {
        // absent selected components initialize without touching their payload; mxcsr is always read.
        selected(code, 6);
        fits(code, 28);
        code.i32_eqz().i32_and();
        memory::add_check(code);
    }
}

fn fault(code: &mut InstructionSink<'_>, save: bool) {
    memory::helper_field(code, 28);
    code.local_get(ADDRESS).i32_sub().local_set(RHS);
    let header = MemorySpan {
        selection: 0,
        offset: 512,
        width: if save { 8 } else { 24 },
    };
    memory::helper_field(code, 24);
    code.i32_const(3)
        .i32_eq()
        .if_(BlockType::Result(ValType::I32));
    span_fault(code, header, 1, true);
    // a payload overflow can occur after a valid header because ymm data follows the header.
    fits(code, header.offset + header.width);
    code.i32_const(0);
    for span in if save { SAVE_SPANS } else { RESTORE_SPANS } {
        span_fault(code, span, if save { 2 } else { 1 }, true);
        code.i32_or();
    }
    code.i32_and().i32_or();
    code.else_();
    code.i32_const(0);
    span_fault(code, header, 1, false);
    code.i32_or();
    for span in if save { SAVE_SPANS } else { RESTORE_SPANS } {
        span_fault(code, span, if save { 2 } else { 1 }, false);
        code.i32_or();
    }
    fits(code, header.offset + header.width);
    code.i32_and();
    code.end().i32_eqz();
    memory::add_check(code);
}

/// address-overflow packets identify either the base (field addition) or field start (span end).
fn span_fault(code: &mut InstructionSink<'_>, span: MemorySpan, access: i32, overflow: bool) {
    selected(code, span.selection);
    memory::helper_field(code, 32);
    code.i32_const(access).i32_eq().i32_and();
    if overflow {
        fits(code, span.offset + span.width);
        code.i32_eqz().i32_and();
        memory::helper_field(code, 28);
        if span.offset != 0 {
            fits(code, span.offset + 1);
            code.if_(BlockType::Result(ValType::I32));
            code.local_get(ADDRESS)
                .i32_const(span.offset as i32)
                .i32_add();
            code.else_().local_get(ADDRESS).end();
        } else {
            code.local_get(ADDRESS);
        }
        code.i32_eq().i32_and();
    } else {
        fits(code, span.offset + span.width);
        code.i32_and();
        code.local_get(RHS)
            .i32_const(span.offset as i32)
            .i32_ge_u()
            .local_get(RHS)
            .i32_const((span.offset + span.width) as i32)
            .i32_lt_u()
            .i32_and()
            .i32_and();
    }
}
