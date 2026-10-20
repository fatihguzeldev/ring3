use super::{
    A, ADDRESS, B, CARRY, EIP, FLAGS, INDEX, KIND, LEFT, LOGICAL_ADDRESS, Output, RESULT, RIGHT,
    VALUE,
};
use crate::cpu::{
    core::StopReason,
    x86::{ControlOperation, Register32, ir::*},
};
use wasm_encoder::BlockType;

pub(super) fn supports(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::PushWord { .. }
            | Operation::PushMemoryWord { .. }
            | Operation::PushFlagsWord
            | Operation::PopWord { .. }
            | Operation::PopMemoryWord { .. }
            | Operation::ReturnWord { .. }
            | Operation::JumpWord { .. }
            | Operation::ReadJumpWord { .. }
            | Operation::CallWord { .. }
            | Operation::CallRegisterWord { .. }
            | Operation::ReadCallWord { .. }
            | Operation::CountBranchWord { .. }
            | Operation::EnterZero
            | Operation::EnterZeroWord
            | Operation::LeaveWord
            | Operation::PopAll
            | Operation::PopAllWord
            | Operation::Control(ControlOperation::PopFlags { .. })
    )
}

pub(super) fn emit(out: &mut Output<'_>, operation: Operation, next: u32) -> bool {
    match operation {
        Operation::PushWord { source } => {
            word(out, source, RESULT);
            push(out, 2, RESULT);
            out.set_pc(next);
        }
        Operation::PushMemoryWord { source } => {
            out.address(source);
            out.read(2, RESULT);
            push(out, 2, RESULT);
            out.set_pc(next);
        }
        Operation::PushFlagsWord => {
            out.materialize(super::super::flags::ARITHMETIC);
            out.c()
                .local_get(FLAGS)
                .i32_const(!0x30000)
                .i32_and()
                .local_set(RESULT);
            push(out, 2, RESULT);
            out.set_pc(next);
        }
        Operation::PopWord { destination } => {
            stack_address(out, 0);
            out.read(2, RESULT);
            out.c().local_get(6).i32_const(2).i32_add().local_set(6);
            store_word(out, destination, RESULT);
            out.set_pc(next);
        }
        Operation::PopMemoryWord { mut destination } => {
            stack_address(out, 0);
            out.read(2, RESULT);
            if destination.base == Some(Register32::Esp) {
                destination.displacement = destination.displacement.wrapping_add(2);
            }
            out.address(destination);
            out.write(2, RESULT);
            out.c().local_get(6).i32_const(2).i32_add().local_set(6);
            out.set_pc(next);
        }
        Operation::ReturnWord { stack_adjust } => {
            stack_address(out, 0);
            out.read(2, RESULT);
            out.c()
                .local_get(6)
                .i32_const(2 + i32::from(stack_adjust))
                .i32_add()
                .local_set(6);
            out.dynamic_pc(RESULT);
        }
        Operation::JumpWord { source } => {
            word(out, WordValue::Register(source), RESULT);
            out.dynamic_pc(RESULT);
        }
        Operation::ReadJumpWord { address } => {
            out.address(address);
            out.read(2, RESULT);
            out.dynamic_pc(RESULT);
        }
        Operation::CallWord { target } => {
            out.c().i32_const((target.0 & 0xffff) as i32).local_set(B);
            call_word(out, next);
        }
        Operation::CallRegisterWord { source } => {
            word(out, WordValue::Register(source), B);
            call_word(out, next);
        }
        Operation::ReadCallWord { address } => {
            out.address(address);
            out.read(2, B);
            call_word(out, next);
        }
        Operation::CountBranchWord { kind, target } => {
            if kind != CountBranchKind::EcxZero {
                out.c()
                    .local_get(3)
                    .i32_const(1)
                    .i32_sub()
                    .i32_const(0xffff)
                    .i32_and()
                    .local_get(3)
                    .i32_const(!0xffff)
                    .i32_and()
                    .i32_or()
                    .local_set(3);
            }
            out.c().local_get(3).i32_const(0xffff).i32_and().i32_eqz();
            if kind != CountBranchKind::EcxZero {
                out.c().i32_eqz();
            }
            out.c().local_set(A);
            if matches!(
                kind,
                CountBranchKind::LoopEqual | CountBranchKind::LoopNotEqual
            ) {
                out.condition(if kind == CountBranchKind::LoopEqual {
                    Condition::Equal
                } else {
                    Condition::NotEqual
                });
                out.c().local_get(A).i32_and().local_set(A);
            }
            let taken = out.index(target.0);
            let otherwise = out.index(next);
            out.c()
                .i32_const(target.0 as i32)
                .i32_const(next as i32)
                .local_get(A)
                .select()
                .local_set(EIP)
                .i32_const(taken)
                .i32_const(otherwise)
                .local_get(A)
                .select()
                .local_set(INDEX);
        }
        Operation::EnterZero | Operation::EnterZeroWord => {
            let width = if operation == Operation::EnterZeroWord {
                2
            } else {
                4
            };
            out.c().local_get(7).local_set(RESULT);
            push(out, width, RESULT);
            if width == 2 {
                store_word(out, Register32::Ebp, 6);
            } else {
                out.c().local_get(6).local_set(7);
            }
            out.set_pc(next);
        }
        Operation::LeaveWord => {
            out.c()
                .local_get(7)
                .local_tee(ADDRESS)
                .local_set(LOGICAL_ADDRESS);
            out.read(2, RESULT);
            out.c().local_get(7).i32_const(2).i32_add().local_set(6);
            store_word(out, Register32::Ebp, RESULT);
            out.set_pc(next);
        }
        Operation::PopAll | Operation::PopAllWord => {
            pop_all(
                out,
                if operation == Operation::PopAllWord {
                    2
                } else {
                    4
                },
            );
            out.set_pc(next);
        }
        Operation::Control(ControlOperation::PopFlags { word }) => {
            let width = if word { 2 } else { 4 };
            stack_address(out, 0);
            out.read(width, RESULT);
            out.materialize(super::super::flags::ARITHMETIC);
            out.c().i32_const(0).local_set(KIND);
            let mask = if word {
                crate::cpu::x86::USER_POP_FLAGS_WORD_MASK
            } else {
                crate::cpu::x86::USER_POP_FLAGS_MASK
            };
            out.c()
                .local_get(FLAGS)
                .i32_const(!(mask | 0x10000) as i32)
                .i32_and()
                .local_get(RESULT)
                .i32_const(mask as i32)
                .i32_and()
                .i32_or()
                .i32_const(2)
                .i32_or()
                .local_set(FLAGS)
                .local_get(6)
                .i32_const(width as i32)
                .i32_add()
                .local_set(6);
            out.set_pc(next);
            out.c()
                .local_get(FLAGS)
                .i32_const(0x50100)
                .i32_and()
                .if_(BlockType::Empty);
            out.exit(StopReason::Debug, Some(next), out.prefix + 1);
            out.c().end();
        }
        _ => return false,
    }
    true
}

fn word(out: &mut Output<'_>, value: WordValue, target: u32) {
    match value {
        WordValue::Register(register) => {
            out.c().local_get(register.index() as u32 + 2);
        }
        WordValue::Immediate(value) => {
            out.c().i32_const(i32::from(value));
        }
    }
    out.c().i32_const(0xffff).i32_and().local_set(target);
}

fn stack_address(out: &mut Output<'_>, offset: u32) {
    out.c()
        .local_get(6)
        .i32_const(offset as i32)
        .i32_add()
        .local_tee(ADDRESS)
        .local_set(LOGICAL_ADDRESS);
}

fn push(out: &mut Output<'_>, width: u32, source: u32) {
    out.c()
        .local_get(6)
        .i32_const(width as i32)
        .i32_sub()
        .local_tee(ADDRESS)
        .local_set(LOGICAL_ADDRESS);
    out.write(width, source);
    out.c().local_get(ADDRESS).local_set(6);
}

fn call_word(out: &mut Output<'_>, next: u32) {
    out.c().i32_const((next & 0xffff) as i32).local_set(RESULT);
    push(out, 2, RESULT);
    out.dynamic_pc(B);
}

fn store_word(out: &mut Output<'_>, destination: Register32, source: u32) {
    let destination = destination.index() as u32 + 2;
    out.c()
        .local_get(destination)
        .i32_const(!0xffff)
        .i32_and()
        .local_get(source)
        .i32_const(0xffff)
        .i32_and()
        .i32_or()
        .local_set(destination);
}

fn pop_all(out: &mut Output<'_>, width: u32) {
    out.materialize(super::super::flags::ARITHMETIC);
    out.c().i32_const(0).local_set(KIND);
    let values = [
        (9, 0, A),
        (8, 1, B),
        (7, 2, RESULT),
        (5, 4, CARRY),
        (4, 5, LEFT),
        (3, 6, RIGHT),
        (2, 7, VALUE),
    ];
    for (_, slot, local) in values {
        stack_address(out, slot * width);
        out.read(width, local);
    }
    for (register, _, local) in values {
        if width == 4 {
            out.c().local_get(local).local_set(register);
        } else {
            out.c()
                .local_get(register)
                .i32_const(!0xffff)
                .i32_and()
                .local_get(local)
                .i32_or()
                .local_set(register);
        }
    }
    out.c()
        .local_get(6)
        .i32_const((width * 8) as i32)
        .i32_add()
        .local_set(6);
}
