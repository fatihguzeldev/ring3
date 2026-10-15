use crate::cpu::dbt::wasm::{helpers::protocol, imports::Imports};
pub(in crate::cpu::dbt::wasm) mod pointer;

use crate::{
    abi::arena::TRANSFER_OFFSET,
    cpu::{
        dbt::wasm::locals::*,
        x86::{Register32, StackFrameKind, decode::DecodedOperation, ir::Operation},
    },
};
use wasm_encoder::{BlockType, InstructionSink, MemArg};
const REGISTERS: [Register32; 8] = [
    Register32::Eax,
    Register32::Ecx,
    Register32::Edx,
    Register32::Ebx,
    Register32::Esp,
    Register32::Ebp,
    Register32::Esi,
    Register32::Edi,
];

pub(in crate::cpu::dbt::wasm) fn instruction(
    code: &mut InstructionSink<'_>,
    instruction: &DecodedOperation,
    imports: Imports,
    exit_depth: u32,
) {
    let (kind, allocation, nesting) = match *instruction.operation() {
        Operation::StackFrame {
            kind,
            allocation,
            nesting,
        } => (kind, allocation, nesting),
        Operation::PopAll => (StackFrameKind::PopAll32, 0, 0),
        Operation::PopAllWord => (StackFrameKind::PopAll16, 0, 0),
        Operation::EnterZero => (StackFrameKind::Enter32, 0, 0),
        Operation::EnterZeroWord => (StackFrameKind::Enter16, 0, 0),
        _ => unreachable!("stack emitter requires stack operation"),
    };
    crate::cpu::dbt::wasm::instructions::segments::access(
        code,
        crate::cpu::x86::SegmentRegister::Ss,
        exit_depth,
    );
    for register in REGISTERS {
        code.local_get(STATE_PTR)
            .local_get(crate::cpu::dbt::wasm::locals::register(register))
            .i32_store(transfer(register.index() * 4));
    }
    let index = imports
        .stack_execute
        .expect("stack operation has a helper")
        .emit_binding(code);
    code.i32_const(kind.control(allocation, nesting) as i32)
        .call(index)
        .local_set(HELPER_STATUS);
    if matches!(kind, StackFrameKind::PopAll32 | StackFrameKind::PopAll16) {
        crate::cpu::dbt::wasm::code_fetch::check_context(code, exit_depth);
    }
    validate(
        code,
        kind,
        allocation,
        nesting,
        exit_depth,
        crate::cpu::dbt::wasm::helpers::result::ResultContext::from(imports),
    );
    let width = kind.width();
    match kind {
        StackFrameKind::PushAll32 | StackFrameKind::PushAll16 => {
            pointer::advance(code, -((8 * width) as i32));
        }
        StackFrameKind::PopAll32 | StackFrameKind::PopAll16 => {
            for destination in REGISTERS.into_iter().filter(|r| *r != Register32::Esp) {
                if width == 2 {
                    code.local_get(register(destination))
                        .i32_const(!0xffff)
                        .i32_and();
                }
                code.local_get(STATE_PTR)
                    .i32_load(transfer(destination.index() * 4));
                if width == 2 {
                    code.i32_const(0xffff).i32_and().i32_or();
                }
                code.local_set(register(destination));
            }
            pointer::advance(code, (8 * width) as i32);
        }
        StackFrameKind::Enter32 | StackFrameKind::Enter16 => {
            if width == 2 {
                code.local_get(register(Register32::Ebp))
                    .i32_const(!0xffff)
                    .i32_and();
            }
            pointer::assigned(code, register(Register32::Esp), -(width as i32));
            if width == 2 {
                code.i32_const(0xffff).i32_and().i32_or();
            }
            code.local_set(register(Register32::Ebp));
            let count = if nesting == 0 {
                1
            } else {
                u32::from(nesting) + 1
            };
            pointer::advance(code, -((count * width + u32::from(allocation)) as i32));
        }
    }
    code.i32_const(instruction.next_pc().0 as i32)
        .local_set(EIP)
        .local_get(BUDGET)
        .i32_const(1)
        .i32_sub()
        .local_set(BUDGET)
        .local_get(RETIRED)
        .i32_const(1)
        .i32_add()
        .local_set(RETIRED);
    if !matches!(kind, StackFrameKind::PopAll32 | StackFrameKind::PopAll16) {
        protocol::exit_if_invalidated(code, exit_depth, imports);
    }
}

fn transfer(offset: usize) -> MemArg {
    MemArg {
        offset: (TRANSFER_OFFSET + offset) as u64,
        align: 2,
        memory_index: 0,
    }
}

fn validate(
    code: &mut InstructionSink<'_>,
    kind: StackFrameKind,
    allocation: u16,
    nesting: u8,
    exit_depth: u32,
    context: crate::cpu::dbt::wasm::helpers::result::ResultContext,
) {
    let writes = !matches!(kind, StackFrameKind::PopAll32 | StackFrameKind::PopAll16);
    if context.paging_configured {
        crate::cpu::dbt::wasm::helpers::result::paging::is_packet(code);
        code.if_(BlockType::Empty);
        crate::cpu::dbt::wasm::helpers::result::paging::transport(code, writes);
        protocol::exit_if_failed(code, exit_depth + 1);
        protocol::reset_checks(code);
        membership(code, kind, allocation, nesting, context);
        code.local_get(RESUME).i32_eqz();
        protocol::add_check(code);
        protocol::protocol_if_bad(code);
        protocol::exit_if_failed(code, exit_depth + 1);
        crate::cpu::dbt::wasm::helpers::result::paging::validate(
            code,
            crate::cpu::dbt::wasm::helpers::result::paging::Validation {
                length: kind.width(),
                access_mask: if matches!(kind, StackFrameKind::PopAll32 | StackFrameKind::PopAll16)
                {
                    1
                } else if matches!(kind, StackFrameKind::PushAll32 | StackFrameKind::PushAll16) {
                    2
                } else {
                    3
                },
                allow_smc: writes,
                geometry: crate::cpu::dbt::wasm::helpers::result::paging::Geometry::OwnerChecked,
            },
        );
        protocol::exit_if_failed(code, exit_depth + 1);
        code.end();
    }
    validate_legacy(code, kind, allocation, nesting, exit_depth);
}

fn validate_legacy(
    code: &mut InstructionSink<'_>,
    kind: StackFrameKind,
    allocation: u16,
    nesting: u8,
    exit_depth: u32,
) {
    use crate::cpu::dbt::wasm::helpers::protocol::{
        add_check, expect_field, helper_field, infrastructure, protocol_if_bad, reset_checks,
    };
    let writes = !matches!(kind, StackFrameKind::PopAll32 | StackFrameKind::PopAll16);
    code.local_get(HELPER_STATUS).i32_eqz();
    if writes {
        code.local_get(HELPER_STATUS)
            .i32_const(11)
            .i32_eq()
            .i32_or();
    }
    code.i32_eqz().if_(BlockType::Empty);
    infrastructure(code, 3);
    code.else_();
    reset_checks(code);
    for (offset, value) in [
        (0, u32::from_le_bytes(*b"R3MH")),
        (4, 0x10007),
        (8, 40),
        (12, 0),
        (20, 0),
    ] {
        expect_field(code, offset, value);
    }
    protocol_if_bad(code);
    helper_field(code, 16);
    code.local_set(RHS);
    code.local_get(REASON).i32_eqz().if_(BlockType::Empty);
    code.local_get(RHS)
        .i32_const(7)
        .i32_eq()
        .if_(BlockType::Empty);
    reset_checks(code);
    for offset in [24, 28, 32] {
        expect_field(code, offset, 0);
    }
    expect_field(code, 36, kind.width());
    code.local_get(HELPER_STATUS).i32_eqz().i32_eqz();
    add_check(code);
    code.local_get(LHS).if_(BlockType::Empty);
    infrastructure(code, 2);
    code.else_();
    code.i32_const(15).local_set(REASON);
    for local in [DETAIL, FAULT_ADDRESS, FAULT_ACCESS, FAULT_LENGTH] {
        code.i32_const(0).local_set(local);
    }
    code.end();
    code.else_();
    code.local_get(RHS).i32_eqz().if_(BlockType::Empty);
    reset_checks(code);
    for offset in [24, 28, 32, 36] {
        expect_field(code, offset, 0);
    }
    protocol_if_bad(code);
    code.else_();
    code.local_get(HELPER_STATUS)
        .i32_const(11)
        .i32_eq()
        .if_(BlockType::Empty);
    infrastructure(code, 2);
    code.else_();
    code.local_get(RHS)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Empty);
    reset_checks(code);
    expect_field(code, 36, kind.width());
    helper_field(code, 24);
    code.i32_const(1).i32_sub().i32_const(1).i32_gt_u();
    add_check(code);
    membership(
        code,
        kind,
        allocation,
        nesting,
        crate::cpu::dbt::wasm::helpers::result::ResultContext::legacy(),
    );
    code.local_get(RESUME).i32_eqz();
    add_check(code);
    code.local_get(LHS).if_(BlockType::Empty);
    infrastructure(code, 2);
    code.else_();
    code.i32_const(5).local_set(REASON);
    for (offset, local) in [
        (24, DETAIL),
        (28, FAULT_ADDRESS),
        (32, FAULT_ACCESS),
        (36, FAULT_LENGTH),
    ] {
        helper_field(code, offset);
        code.local_set(local);
    }
    code.end();
    code.else_();
    code.local_get(RHS)
        .i32_const(2)
        .i32_eq()
        .if_(BlockType::Empty);
    reset_checks(code);
    for offset in [28, 32, 36] {
        expect_field(code, offset, 0);
    }
    helper_field(code, 24);
    code.i32_const(1).i32_sub().i32_const(1).i32_gt_u();
    add_check(code);
    code.local_get(LHS).if_(BlockType::Empty);
    infrastructure(code, 2);
    code.else_();
    helper_field(code, 24);
    code.i32_const(1).i32_eq().if_(BlockType::Empty);
    infrastructure(code, 1);
    code.else_();
    infrastructure(code, 3);
    code.end().end();
    code.else_();
    infrastructure(code, 2);
    code.end().end().end().end().end().end().end();
    protocol::exit_if_failed(code, exit_depth);
}

fn membership(
    code: &mut InstructionSink<'_>,
    kind: StackFrameKind,
    allocation: u16,
    nesting: u8,
    context: crate::cpu::dbt::wasm::helpers::result::ResultContext,
) {
    let width = kind.width();
    code.i32_const(0).local_set(RESUME);
    match kind {
        StackFrameKind::PushAll32 | StackFrameKind::PushAll16 => {
            for index in 1..=8 {
                candidate(
                    code,
                    Register32::Esp,
                    0u32.wrapping_sub(index * width),
                    width,
                    2,
                    context,
                );
            }
        }
        StackFrameKind::PopAll32 | StackFrameKind::PopAll16 => {
            for index in [0, 1, 2, 4, 5, 6, 7] {
                candidate(code, Register32::Esp, index * width, width, 1, context);
            }
        }
        StackFrameKind::Enter32 | StackFrameKind::Enter16 => {
            let count = if nesting == 0 {
                1
            } else {
                u32::from(nesting) + 1
            };
            for index in 1..=count {
                candidate(
                    code,
                    Register32::Esp,
                    0u32.wrapping_sub(index * width),
                    width,
                    2,
                    context,
                );
            }
            for index in 1..u32::from(nesting) {
                candidate(
                    code,
                    Register32::Ebp,
                    0u32.wrapping_sub(index * width),
                    width,
                    1,
                    context,
                );
            }
            candidate(
                code,
                Register32::Esp,
                0u32.wrapping_sub(count * width + u32::from(allocation)),
                width,
                2,
                context,
            );
        }
    }
}

fn candidate(
    code: &mut InstructionSink<'_>,
    register: Register32,
    delta: u32,
    width: u32,
    access: u32,

    context: crate::cpu::dbt::wasm::helpers::result::ResultContext,
) {
    use protocol::helper_field;
    pointer::offset(
        code,
        crate::cpu::dbt::wasm::locals::register(register),
        delta as i32,
    );
    crate::cpu::dbt::wasm::instructions::segments::base(code, crate::cpu::x86::SegmentRegister::Ss);
    code.i32_add().local_set(ADDRESS);
    code.local_get(RESUME);
    helper_field(code, 28);
    code.local_get(ADDRESS)
        .i32_sub()
        .i32_const(width as i32)
        .i32_lt_u();
    if context.paging_configured {
        crate::cpu::dbt::wasm::helpers::result::paging::intent(code);
    } else {
        helper_field(code, 32);
    }
    code.i32_const(access as i32)
        .i32_eq()
        .i32_and()
        .i32_or()
        .local_set(RESUME);
}
