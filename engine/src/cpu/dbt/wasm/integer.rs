use wasm_encoder::{BlockType, InstructionSink, ValType};

use super::{control, locals::*};
use crate::cpu::x86::{
    decode::DecodedInstruction,
    ir::{BinaryKind, BranchTarget, Location32, Operation, Value32},
};

pub(super) fn instruction(code: &mut InstructionSink<'_>, instruction: &DecodedInstruction) {
    match *instruction.operation() {
        Operation::Nop => {}
        Operation::Move {
            destination: Location32::Register(destination),
            source,
        } => {
            value(code, source);
            code.local_set(register(destination));
        }
        Operation::Binary {
            kind: kind @ (BinaryKind::Add | BinaryKind::Sub | BinaryKind::Cmp),
            destination: Location32::Register(destination),
            source,
        } => {
            code.local_get(register(destination)).local_set(LHS);
            value(code, source);
            code.local_set(RHS).local_get(LHS).local_get(RHS);
            if kind == BinaryKind::Add {
                code.i32_add();
            } else {
                code.i32_sub();
            }
            code.local_set(RESULT);
            flags(code, kind);
            if kind != BinaryKind::Cmp {
                code.local_get(RESULT).local_set(register(destination));
            }
        }
        Operation::Jump {
            target: BranchTarget::Direct(target),
        } => {
            code.i32_const(target.0 as i32).local_set(EIP);
        }
        Operation::ConditionalJump { condition, target } => {
            control::condition(code, condition);
            code.if_(BlockType::Result(ValType::I32))
                .i32_const(target.0 as i32)
                .else_()
                .i32_const(instruction.next_pc().0 as i32)
                .end()
                .local_set(EIP);
        }
        _ => unreachable!("prepared region contains an unsupported operation"),
    }
    if !matches!(
        instruction.operation(),
        Operation::Jump { .. } | Operation::ConditionalJump { .. }
    ) {
        code.i32_const(instruction.next_pc().0 as i32)
            .local_set(EIP);
    }
    code.local_get(BUDGET)
        .i32_const(1)
        .i32_sub()
        .local_set(BUDGET)
        .local_get(RETIRED)
        .i32_const(1)
        .i32_add()
        .local_set(RETIRED);
}

fn value(code: &mut InstructionSink<'_>, value: Value32) {
    match value {
        Value32::Register(source) => {
            code.local_get(register(source));
        }
        Value32::Immediate(value) => {
            code.i32_const(value as i32);
        }
        Value32::Memory(_) => unreachable!("prepared region contains a memory operand"),
    }
}

fn flags(code: &mut InstructionSink<'_>, kind: BinaryKind) {
    code.local_get(FLAGS)
        .i32_const(0x400)
        .i32_and()
        .i32_const(2)
        .i32_or();
    if kind == BinaryKind::Add {
        code.local_get(RESULT).local_get(LHS).i32_lt_u();
    } else {
        code.local_get(LHS).local_get(RHS).i32_lt_u();
    }
    code.i32_or()
        .local_get(RESULT)
        .i32_const(0xff)
        .i32_and()
        .i32_popcnt()
        .i32_const(1)
        .i32_and()
        .i32_eqz()
        .i32_const(2)
        .i32_shl()
        .i32_or()
        .local_get(LHS)
        .local_get(RHS)
        .i32_xor()
        .local_get(RESULT)
        .i32_xor()
        .i32_const(0x10)
        .i32_and()
        .i32_or()
        .local_get(RESULT)
        .i32_eqz()
        .i32_const(6)
        .i32_shl()
        .i32_or()
        .local_get(RESULT)
        .i32_const(24)
        .i32_shr_u()
        .i32_const(0x80)
        .i32_and()
        .i32_or()
        .local_get(LHS)
        .local_get(RHS)
        .i32_xor();
    if kind == BinaryKind::Add {
        code.i32_const(-1).i32_xor();
    }
    code.local_get(LHS)
        .local_get(RESULT)
        .i32_xor()
        .i32_and()
        .i32_const(31)
        .i32_shr_u()
        .i32_const(11)
        .i32_shl()
        .i32_or()
        .local_set(FLAGS);
}
