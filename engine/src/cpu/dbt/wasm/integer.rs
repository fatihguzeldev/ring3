use wasm_encoder::{BlockType, InstructionSink, ValType};

use super::{control, locals::*, memory};
use crate::cpu::x86::{
    Register32,
    decode::DecodedInstruction,
    ir::{BinaryKind, BranchTarget, EffectiveAddress, Location32, Operation, Value32},
};

pub(super) fn instruction(
    code: &mut InstructionSink<'_>,
    instruction: &DecodedInstruction,
    imports: memory::Imports,
    exit_depth: u32,
) {
    let mut store = false;
    match *instruction.operation() {
        Operation::Nop => {}
        Operation::Move {
            destination: Location32::Register(destination),
            source,
        } => {
            if let Value32::Memory(address) = source {
                memory::load(code, address, destination, imports, exit_depth);
            } else {
                value(code, source);
                code.local_set(register(destination));
            }
        }
        Operation::Move {
            destination: Location32::Memory(address),
            source,
        } => {
            memory::store(code, address, source, imports, exit_depth);
            store = true;
        }
        Operation::Binary {
            kind,
            destination: Location32::Register(destination),
            source,
        } => {
            code.local_get(register(destination)).local_set(LHS);
            value(code, source);
            code.local_set(RHS).local_get(LHS).local_get(RHS);
            match kind {
                BinaryKind::Add => {
                    code.i32_add();
                }
                BinaryKind::Sub | BinaryKind::Cmp => {
                    code.i32_sub();
                }
                BinaryKind::And | BinaryKind::Test => {
                    code.i32_and();
                }
                BinaryKind::Or => {
                    code.i32_or();
                }
                BinaryKind::Xor => {
                    code.i32_xor();
                }
            }
            code.local_set(RESULT);
            match kind {
                BinaryKind::Add | BinaryKind::Sub | BinaryKind::Cmp => arithmetic_flags(code, kind),
                BinaryKind::And | BinaryKind::Or | BinaryKind::Xor | BinaryKind::Test => {
                    logical_flags(code)
                }
            }
            if !matches!(kind, BinaryKind::Cmp | BinaryKind::Test) {
                code.local_get(RESULT).local_set(register(destination));
            }
        }
        Operation::Jump {
            target: BranchTarget::Direct(target),
        } => {
            code.i32_const(target.0 as i32).local_set(EIP);
        }
        Operation::Jump {
            target: BranchTarget::Indirect(target),
        } => {
            indirect_target(code, target, imports, exit_depth);
            code.local_get(RESULT).local_set(EIP);
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
        Operation::Call {
            target: BranchTarget::Direct(target),
        } => {
            memory::store(
                code,
                EffectiveAddress {
                    base: Some(Register32::Esp),
                    index: None,
                    scale: 1,
                    displacement: (-4_i32) as u32,
                },
                Value32::Immediate(instruction.next_pc().0),
                imports,
                exit_depth,
            );
            code.local_get(ADDRESS)
                .local_set(register(Register32::Esp))
                .i32_const(target.0 as i32)
                .local_set(EIP);
            store = true;
        }
        Operation::Call {
            target: BranchTarget::Indirect(target),
        } => {
            indirect_target(code, target, imports, exit_depth);
            memory::store(
                code,
                EffectiveAddress {
                    base: Some(Register32::Esp),
                    index: None,
                    scale: 1,
                    displacement: (-4_i32) as u32,
                },
                Value32::Immediate(instruction.next_pc().0),
                imports,
                exit_depth,
            );
            code.local_get(ADDRESS)
                .local_set(register(Register32::Esp))
                .local_get(RESULT)
                .local_set(EIP);
            store = true;
        }
        Operation::Push {
            source: Value32::Memory(source),
        } => {
            memory::push_memory(code, source, imports, exit_depth);
            store = true;
        }
        Operation::Push { source } => {
            memory::store(
                code,
                EffectiveAddress {
                    base: Some(Register32::Esp),
                    index: None,
                    scale: 1,
                    displacement: (-4_i32) as u32,
                },
                source,
                imports,
                exit_depth,
            );
            code.local_get(ADDRESS).local_set(register(Register32::Esp));
            store = true;
        }
        Operation::Pop {
            destination: Location32::Register(destination),
        } => {
            memory::pop_register(code, destination, imports, exit_depth);
        }
        Operation::Pop {
            destination: Location32::Memory(destination),
        } => {
            memory::pop_memory(code, destination, imports, exit_depth);
            store = true;
        }
        Operation::Return { stack_adjust } => {
            memory::pop_return(code, stack_adjust, imports, exit_depth);
        }
        _ => unreachable!("prepared region contains an unsupported operation"),
    }
    if !matches!(
        instruction.operation(),
        Operation::Jump { .. }
            | Operation::ConditionalJump { .. }
            | Operation::Call { .. }
            | Operation::Return { .. }
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
    if store {
        memory::exit_if_invalidated(code, exit_depth);
    }
}

fn indirect_target(
    code: &mut InstructionSink<'_>,
    target: Location32,
    imports: memory::Imports,
    exit_depth: u32,
) {
    match target {
        Location32::Register(target) => {
            code.local_get(register(target)).local_set(RESULT);
        }
        Location32::Memory(address) => {
            memory::load_result(code, address, imports, exit_depth);
        }
    }
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

fn arithmetic_flags(code: &mut InstructionSink<'_>, kind: BinaryKind) {
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
    code.i32_or();
    parity_flag(code);
    code.local_get(LHS)
        .local_get(RHS)
        .i32_xor()
        .local_get(RESULT)
        .i32_xor()
        .i32_const(0x10)
        .i32_and()
        .i32_or();
    zero_sign_flags(code);
    code.local_get(LHS).local_get(RHS).i32_xor();
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

fn logical_flags(code: &mut InstructionSink<'_>) {
    // af is undefined on x86; this profile deterministically clears it with cf/of.
    code.local_get(FLAGS)
        .i32_const(0x400)
        .i32_and()
        .i32_const(2)
        .i32_or();
    parity_flag(code);
    zero_sign_flags(code);
    code.local_set(FLAGS);
}

fn parity_flag(code: &mut InstructionSink<'_>) {
    code.local_get(RESULT)
        .i32_const(0xff)
        .i32_and()
        .i32_popcnt()
        .i32_const(1)
        .i32_and()
        .i32_eqz()
        .i32_const(2)
        .i32_shl()
        .i32_or();
}

fn zero_sign_flags(code: &mut InstructionSink<'_>) {
    code.local_get(RESULT)
        .i32_eqz()
        .i32_const(6)
        .i32_shl()
        .i32_or()
        .local_get(RESULT)
        .i32_const(24)
        .i32_shr_u()
        .i32_const(0x80)
        .i32_and()
        .i32_or();
}
