use wasm_encoder::{BlockType, InstructionSink};

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
    context: super::super::cpu_result::ResultContext,
) {
    if context.paging_configured {
        super::super::cpu_result::paging::is_packet(code);
        code.if_(BlockType::Empty);
        super::super::cpu_result::paging::transport(code, save);
        memory::exit_if_failed(code, exit_depth + 1);
        memory::reset_checks(code);
        code.local_get(ADDRESS).i32_const(63).i32_and();
        memory::add_check(code);
        fault(code, save, true);
        memory::protocol_if_bad(code);
        memory::exit_if_failed(code, exit_depth + 1);
        code.else_();
        validate_legacy_context(code, save, segment, exit_depth + 1);
        code.end();
    } else {
        validate_legacy_context(code, save, segment, exit_depth);
    }
}

fn validate_legacy_context(
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
    code.i32_const(1).i32_eq().if_(BlockType::Empty);
    fault(code, save, false);
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

fn fault(code: &mut InstructionSink<'_>, save: bool, typed: bool) {
    memory::helper_field(code, 28);
    code.local_get(ADDRESS).i32_sub().local_set(RHS);
    let header = MemorySpan {
        selection: 0,
        offset: 512,
        width: if save { 8 } else { 24 },
    };
    code.i32_const(0);
    span_fault(code, header, 1, typed);
    code.i32_or();
    for span in if save { SAVE_SPANS } else { RESTORE_SPANS } {
        span_fault(code, span, if save { 2 } else { 1 }, typed);
        code.i32_or();
    }
    code.i32_eqz();
    memory::add_check(code);
}

/// the modulo difference identifies a byte in an actually selected xstate span.
fn span_fault(code: &mut InstructionSink<'_>, span: MemorySpan, access: i32, typed: bool) {
    selected(code, span.selection);
    if typed {
        super::super::cpu_result::paging::intent(code);
    } else {
        memory::helper_field(code, 32);
    }
    code.i32_const(access).i32_eq().i32_and();
    code.local_get(RHS)
        .i32_const(span.offset as i32)
        .i32_ge_u()
        .local_get(RHS)
        .i32_const((span.offset + span.width) as i32)
        .i32_lt_u()
        .i32_and()
        .i32_and();
}
