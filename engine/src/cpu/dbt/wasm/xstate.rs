use crate::cpu::dbt::wasm::helpers::protocol;
use crate::cpu::dbt::wasm::imports::Imports;
use wasm_encoder::{BlockType, InstructionSink};

use super::{cpu_result, locals::*, memory, segments};
use crate::cpu::x86::{
    Register32, SegmentAccess, SegmentRegister,
    decode::DecodedInstruction,
    ir::{EffectiveAddress, Operation},
    xstate::{
        XStateOperation,
        access::{MemorySpan, RESTORE_SPANS, SAVE_SPANS},
    },
};

pub(super) fn instruction(
    code: &mut InstructionSink<'_>,
    instruction: &DecodedInstruction,
    imports: Imports,
    exit_depth: u32,
) -> bool {
    let Operation::XState(operation) = instruction.operation() else {
        unreachable!()
    };
    let (address, save) = match operation {
        XStateOperation::ReadControl => {
            code.local_get(register(Register32::Ecx))
                .if_(BlockType::Empty);
            code.i32_const(11).local_set(REASON);
            for local in [DETAIL, FAULT_ADDRESS, FAULT_ACCESS, FAULT_LENGTH] {
                code.i32_const(0).local_set(local);
            }
            code.br(exit_depth + 1).end();
            code.i32_const(crate::cpu::x86::capabilities::XCR0 as i32)
                .local_set(register(Register32::Eax))
                .i32_const(0)
                .local_set(register(Register32::Edx));
            return false;
        }
        XStateOperation::Save { address } => (address, true),
        XStateOperation::Restore { address } => (address, false),
    };
    if crate::cpu::x86::capabilities::AVX_ENABLED {
        upper_instruction(code, *address, save, imports, exit_depth);
        return save;
    }
    check_selected_spans(code, *address, save, imports, exit_depth);
    memory::effective_address(code, *address);
    let helper = if save {
        imports.cpu_xsave
    } else {
        imports.cpu_xrstor
    }
    .expect("xstate helper import");
    let index = helper.emit_binding(code);
    code.local_get(ADDRESS)
        .local_get(register(Register32::Eax))
        .local_get(register(Register32::Edx))
        .call(index)
        .local_set(HELPER_STATUS);
    validate_context(
        code,
        save,
        exit_depth,
        cpu_result::ResultContext::from(imports),
    );
    cpu_result::validate(
        code,
        576,
        if save { 3 } else { 1 },
        save,
        exit_depth,
        cpu_result::ResultContext::from(imports),
    );
    save
}

/// segment authority precedes helper effects; selected init-state restores keep the same page reads.
/// competing segment/header/alignment faults follow ring3's deterministic upfront authority policy.
fn check_selected_spans(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    save: bool,
    imports: Imports,
    exit_depth: u32,
) {
    let spans = if save { &SAVE_SPANS } else { &RESTORE_SPANS };
    let access = if save {
        SegmentAccess::Write
    } else {
        SegmentAccess::Read
    };
    memory::offset_value(code, address);
    code.local_set(LHS);
    for &MemorySpan {
        offset,
        width,
        selection,
    } in spans
    {
        if selection != 0 {
            code.local_get(register(Register32::Eax))
                .i32_const(selection as i32)
                .i32_and()
                .if_(BlockType::Empty);
        }
        let depth = exit_depth + u32::from(selection != 0);
        code.local_get(LHS)
            .i32_const(offset as i32)
            .i32_add()
            .local_set(ADDRESS);
        if offset != 0 {
            // address-size truncation applies to the initial effective offset, never to a field.
            code.local_get(ADDRESS)
                .local_get(LHS)
                .i32_lt_u()
                .if_(BlockType::Empty);
            code.i32_const(if address.resolved_segment() == SegmentRegister::Ss {
                15
            } else {
                11
            })
            .local_set(REASON);
            for local in [DETAIL, FAULT_ADDRESS, FAULT_ACCESS, FAULT_LENGTH] {
                code.i32_const(0).local_set(local);
            }
            code.br(depth + 1).end();
        }
        segments::check_span(
            code,
            address.resolved_segment(),
            width,
            access,
            imports,
            depth,
        );
        if selection != 0 {
            code.end();
        }
    }
}

fn range(code: &mut InstructionSink<'_>, start: i32, end: i32) {
    code.local_get(RHS)
        .i32_const(start)
        .i32_ge_u()
        .local_get(RHS)
        .i32_const(end)
        .i32_lt_u()
        .i32_and();
}

fn selected(code: &mut InstructionSink<'_>, bit: i32) {
    code.local_get(register(Register32::Eax))
        .i32_const(bit)
        .i32_and()
        .i32_eqz()
        .i32_eqz();
}

/// the generic packet envelope cannot authorize holes or unrequested components.
fn validate_context(
    code: &mut InstructionSink<'_>,
    save: bool,
    exit_depth: u32,
    context: super::cpu_result::ResultContext,
) {
    if context.paging_configured {
        super::cpu_result::paging::is_packet(code);
        code.if_(BlockType::Empty);
        super::cpu_result::paging::transport(code, save);
        protocol::exit_if_failed(code, exit_depth + 1);
        protocol::reset_checks(code);
        code.local_get(ADDRESS).i32_const(63).i32_and();
        protocol::add_check(code);
        fault_geometry(code, save, true);
        protocol::protocol_if_bad(code);
        protocol::exit_if_failed(code, exit_depth + 1);
        code.else_();
        validate_legacy_context(code, save, exit_depth + 1);
        code.end();
    } else {
        validate_legacy_context(code, save, exit_depth);
    }
}

fn validate_legacy_context(code: &mut InstructionSink<'_>, save: bool, exit_depth: u32) {
    protocol::reset_checks(code);
    protocol::expect_field(code, 20, 0);
    protocol::helper_field(code, 16);
    code.i32_const(3).i32_gt_u();
    protocol::add_check(code);
    protocol::helper_field(code, 16);
    code.i32_const(1).i32_le_u().if_(BlockType::Empty);
    code.local_get(ADDRESS).i32_const(63).i32_and();
    protocol::add_check(code);
    code.end();
    protocol::helper_field(code, 16);
    code.i32_const(3).i32_eq().if_(BlockType::Empty);
    protocol::expect_field(code, 24, 0);
    code.end();
    protocol::helper_field(code, 16);
    code.i32_const(1).i32_eq().if_(BlockType::Empty);
    fault_geometry(code, save, false);
    code.end();
    protocol::protocol_if_bad(code);
    protocol::exit_if_failed(code, exit_depth);
}

/// the extended helper owns selected cached spans and page transactions as one operation.
fn upper_instruction(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    save: bool,
    imports: Imports,
    exit_depth: u32,
) {
    let helper = if save {
        imports.cpu_xsave
    } else {
        imports.cpu_xrstor
    }
    .expect("extended xstate helper import");
    let index = helper.emit_binding(code);
    code.i32_const(address.resolved_segment().index() as i32);
    memory::offset_value(code, address);
    code.local_get(register(Register32::Eax))
        .local_get(register(Register32::Edx))
        .call(index)
        .local_set(HELPER_STATUS);
    // reconstruct the linear base only for response validation, after the helper's authority checks.
    memory::effective_address(code, address);
    upper::validate_context(
        code,
        save,
        address.resolved_segment(),
        exit_depth,
        cpu_result::ResultContext::from(imports),
    );
    cpu_result::validate_xstate(
        code,
        save,
        exit_depth,
        cpu_result::ResultContext::from(imports),
    );
}

mod upper;

fn fault_geometry(code: &mut InstructionSink<'_>, save: bool, typed: bool) {
    protocol::helper_field(code, 28);
    code.local_get(ADDRESS).i32_sub().local_set(RHS);
    range(code, 512, if save { 520 } else { 536 });
    if save {
        if typed {
            super::cpu_result::paging::intent(code);
        } else {
            protocol::helper_field(code, 32);
        }
        code.i32_const(2).i32_eq();
    }
    selected(code, 1);
    range(code, 0, 24);
    range(code, 32, 160);
    code.i32_or().i32_and();
    selected(code, 2);
    range(code, 24, if save { 32 } else { 28 });
    range(code, 160, 288);
    code.i32_or().i32_and().i32_or();
    if save {
        code.i32_and();
    }
    code.i32_or().i32_eqz();
    protocol::add_check(code);
}
