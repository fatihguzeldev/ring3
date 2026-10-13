pub(super) mod pointer;

use super::{locals::*, memory};
use crate::{
    abi::arena::TRANSFER_OFFSET,
    cpu::x86::{Register32, StackFrameKind, decode::DecodedInstruction, ir::Operation},
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

pub(super) fn instruction(
    code: &mut InstructionSink<'_>,
    instruction: &DecodedInstruction,
    imports: memory::Imports,
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
    super::segments::access(code, crate::cpu::x86::SegmentRegister::Ss, exit_depth);
    for register in REGISTERS {
        code.local_get(STATE_PTR)
            .local_get(super::locals::register(register))
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
        super::code_fetch::check_context(code, exit_depth);
    }
    validate(code, kind, allocation, nesting, exit_depth);
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
        memory::exit_if_invalidated(code, exit_depth);
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
) {
    use memory::{
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
    code.i32_const(1).i32_sub().i32_const(2).i32_gt_u();
    add_check(code);
    membership(code, kind, allocation, nesting);
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
    memory::exit_if_failed(code, exit_depth);
}

fn membership(code: &mut InstructionSink<'_>, kind: StackFrameKind, allocation: u16, nesting: u8) {
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
                );
            }
        }
        StackFrameKind::PopAll32 | StackFrameKind::PopAll16 => {
            for index in [0, 1, 2, 4, 5, 6, 7] {
                candidate(code, Register32::Esp, index * width, width, 1);
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
                );
            }
            for index in 1..u32::from(nesting) {
                candidate(
                    code,
                    Register32::Ebp,
                    0u32.wrapping_sub(index * width),
                    width,
                    1,
                );
            }
            candidate(
                code,
                Register32::Esp,
                0u32.wrapping_sub(count * width + u32::from(allocation)),
                width,
                2,
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
) {
    use memory::helper_field;
    pointer::offset(code, super::locals::register(register), delta as i32);
    super::segments::base(code, crate::cpu::x86::SegmentRegister::Ss);
    code.i32_add().local_set(ADDRESS);
    code.local_get(RESUME);
    helper_field(code, 24);
    code.i32_const(3)
        .i32_eq()
        .if_(BlockType::Result(wasm_encoder::ValType::I32));
    code.local_get(ADDRESS)
        .i32_const(-(width as i32))
        .i32_gt_u();
    helper_field(code, 28);
    code.local_get(ADDRESS).i32_eq().i32_and();
    code.else_();
    code.local_get(ADDRESS)
        .i32_const(-(width as i32))
        .i32_le_u();
    helper_field(code, 28);
    code.local_get(ADDRESS)
        .i32_sub()
        .i32_const(width as i32)
        .i32_lt_u()
        .i32_and();
    code.end();
    helper_field(code, 32);
    code.i32_const(access as i32)
        .i32_eq()
        .i32_and()
        .i32_or()
        .local_set(RESUME);
}
