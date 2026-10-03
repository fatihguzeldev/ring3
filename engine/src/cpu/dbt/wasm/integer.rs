use wasm_encoder::{BlockType, InstructionSink, ValType};

use super::{control, locals::*, memory};
use crate::cpu::x86::{
    Register32,
    decode::DecodedInstruction,
    ir::{
        BinaryKind, BranchTarget, EffectiveAddress, ExtensionKind, Location32, Operation,
        ShiftCount, ShiftKind, SmallSource, SmallWidth, UnaryKind, Value32,
    },
};

#[derive(Clone, Copy)]
enum CarryFlag {
    Calculate,
    Preserve,
}

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
        Operation::Extend {
            kind,
            destination,
            source,
        } => {
            let width = match source {
                SmallSource::Register {
                    register: source,
                    width,
                    high_byte,
                } => {
                    code.local_get(register(source));
                    if high_byte {
                        code.i32_const(8).i32_shr_u();
                    }
                    width
                }
                SmallSource::Memory { address, width } => {
                    memory::load_narrow_value(code, address, width, imports, exit_depth);
                    width
                }
            };
            extend_value(code, kind, width);
            code.local_set(register(destination));
        }
        Operation::Lea {
            destination,
            address,
        } => {
            memory::address_value(code, address);
            code.local_set(register(destination));
        }
        Operation::Binary {
            kind,
            destination,
            source,
        } => {
            match destination {
                Location32::Register(destination) => {
                    if let Value32::Memory(address) = source {
                        memory::load_result(code, address, imports, exit_depth);
                        // helper validation uses operand scratch; capture operands after it succeeds.
                        code.local_get(register(destination))
                            .local_set(LHS)
                            .local_get(RESULT)
                            .local_set(RHS);
                    } else {
                        code.local_get(register(destination)).local_set(LHS);
                        value(code, source);
                        code.local_set(RHS);
                    }
                }
                Location32::Memory(address) => {
                    memory::load_result(code, address, imports, exit_depth);
                    code.local_get(RESULT).local_set(LHS);
                    value(code, source);
                    code.local_set(RHS);
                }
            }
            code.local_get(LHS).local_get(RHS);
            match kind {
                BinaryKind::Add | BinaryKind::Adc => {
                    code.i32_add();
                }
                BinaryKind::Sub | BinaryKind::Sbb | BinaryKind::Cmp => {
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
            if matches!(kind, BinaryKind::Adc | BinaryKind::Sbb) {
                code.local_get(FLAGS).i32_const(1).i32_and();
                if kind == BinaryKind::Adc {
                    code.i32_add();
                } else {
                    code.i32_sub();
                }
            }
            code.local_set(RESULT);
            let writes = !matches!(kind, BinaryKind::Cmp | BinaryKind::Test);
            if writes && let Location32::Memory(address) = destination {
                binary_memory_store(code, kind, address, source, imports, exit_depth);
                store = true;
            }
            match kind {
                BinaryKind::Add
                | BinaryKind::Adc
                | BinaryKind::Sub
                | BinaryKind::Sbb
                | BinaryKind::Cmp => arithmetic_flags(code, kind, CarryFlag::Calculate),
                BinaryKind::And | BinaryKind::Or | BinaryKind::Xor | BinaryKind::Test => {
                    logical_flags(code)
                }
            }
            if writes && let Location32::Register(destination) = destination {
                code.local_get(RESULT).local_set(register(destination));
            }
        }
        Operation::Unary {
            kind,
            destination: Location32::Register(destination),
        } => unary(code, kind, destination),
        Operation::Unary {
            kind,
            destination: Location32::Memory(address),
        } => {
            unary_memory(code, kind, address, imports, exit_depth);
            store = true;
        }
        Operation::Shift {
            kind,
            destination,
            count,
        } => {
            let Location32::Register(destination) = destination else {
                unreachable!("prepared region contains a memory shift")
            };
            shift(code, kind, destination, count);
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

fn extend_value(code: &mut InstructionSink<'_>, kind: ExtensionKind, width: SmallWidth) {
    let bits = match width {
        SmallWidth::Byte => 8,
        SmallWidth::Word => 16,
    };
    match kind {
        ExtensionKind::Zero => {
            code.i32_const((1 << bits) - 1).i32_and();
        }
        ExtensionKind::Sign => {
            code.i32_const(32 - bits)
                .i32_shl()
                .i32_const(32 - bits)
                .i32_shr_s();
        }
    }
}

fn binary_memory_store(
    code: &mut InstructionSink<'_>,
    kind: BinaryKind,
    address: EffectiveAddress,
    source: Value32,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::store_result(code, address, imports, exit_depth);
    if matches!(
        kind,
        BinaryKind::Add | BinaryKind::Adc | BinaryKind::Sub | BinaryKind::Sbb
    ) {
        // store validation uses operand scratch; recover the old destination from the unchanged source.
        value(code, source);
        code.local_set(RHS).local_get(RESULT).local_get(RHS);
        if matches!(kind, BinaryKind::Add | BinaryKind::Adc) {
            code.i32_sub();
        } else {
            code.i32_add();
        }
        if matches!(kind, BinaryKind::Adc | BinaryKind::Sbb) {
            code.local_get(FLAGS).i32_const(1).i32_and();
            if kind == BinaryKind::Adc {
                code.i32_sub();
            } else {
                code.i32_add();
            }
        }
        code.local_set(LHS);
    }
}

fn unary_memory(
    code: &mut InstructionSink<'_>,
    kind: UnaryKind,
    address: EffectiveAddress,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::load_result(code, address, imports, exit_depth);
    match kind {
        UnaryKind::Inc => {
            code.local_get(RESULT).i32_const(1).i32_add();
        }
        UnaryKind::Dec => {
            code.local_get(RESULT).i32_const(1).i32_sub();
        }
        UnaryKind::Not => {
            code.local_get(RESULT).i32_const(-1).i32_xor();
        }
        UnaryKind::Neg => {
            code.i32_const(0).local_get(RESULT).i32_sub();
        }
    }
    code.local_set(RESULT);
    memory::store_result(code, address, imports, exit_depth);

    // store validation uses operand scratch; recover reversible operands after it succeeds.
    let (binary, carry) = match kind {
        UnaryKind::Not => return,
        UnaryKind::Inc | UnaryKind::Dec => {
            code.local_get(RESULT).i32_const(1);
            let binary = if kind == UnaryKind::Inc {
                code.i32_sub();
                BinaryKind::Add
            } else {
                code.i32_add();
                BinaryKind::Sub
            };
            code.local_set(LHS).i32_const(1).local_set(RHS);
            (binary, CarryFlag::Preserve)
        }
        UnaryKind::Neg => {
            code.i32_const(0)
                .local_set(LHS)
                .i32_const(0)
                .local_get(RESULT)
                .i32_sub()
                .local_set(RHS);
            (BinaryKind::Sub, CarryFlag::Calculate)
        }
    };
    arithmetic_flags(code, binary, carry);
}

fn unary(code: &mut InstructionSink<'_>, kind: UnaryKind, destination: Register32) {
    code.local_get(register(destination)).local_set(LHS);
    let (binary, carry) = match kind {
        UnaryKind::Not => {
            code.local_get(LHS)
                .i32_const(-1)
                .i32_xor()
                .local_set(register(destination));
            return;
        }
        UnaryKind::Inc | UnaryKind::Dec => {
            code.i32_const(1).local_set(RHS);
            (
                if kind == UnaryKind::Inc {
                    BinaryKind::Add
                } else {
                    BinaryKind::Sub
                },
                CarryFlag::Preserve,
            )
        }
        UnaryKind::Neg => {
            code.local_get(LHS)
                .local_set(RHS)
                .i32_const(0)
                .local_set(LHS);
            (BinaryKind::Sub, CarryFlag::Calculate)
        }
    };
    code.local_get(LHS).local_get(RHS);
    if binary == BinaryKind::Add {
        code.i32_add();
    } else {
        code.i32_sub();
    }
    code.local_set(RESULT);
    arithmetic_flags(code, binary, carry);
    code.local_get(RESULT).local_set(register(destination));
}

fn shift(
    code: &mut InstructionSink<'_>,
    kind: ShiftKind,
    destination: Register32,
    count: ShiftCount,
) {
    code.local_get(register(destination)).local_set(LHS);
    match count {
        ShiftCount::Immediate(count) => {
            code.i32_const(i32::from(count));
        }
        ShiftCount::Cl => {
            code.local_get(register(Register32::Ecx));
        }
    }
    code.i32_const(31)
        .i32_and()
        .local_tee(RHS)
        .if_(BlockType::Empty)
        .local_get(LHS)
        .local_get(RHS);
    match kind {
        ShiftKind::Shl => {
            code.i32_shl();
        }
        ShiftKind::Shr => {
            code.i32_shr_u();
        }
        ShiftKind::Sar => {
            code.i32_shr_s();
        }
    }
    code.local_set(RESULT);
    // af and multi-bit of are undefined; this profile clears them.
    logical_flags(code);
    code.local_get(FLAGS).local_get(LHS);
    if kind == ShiftKind::Shl {
        code.i32_const(32).local_get(RHS).i32_sub();
    } else {
        code.local_get(RHS).i32_const(1).i32_sub();
    }
    code.i32_shr_u()
        .i32_const(1)
        .i32_and()
        .i32_or()
        .local_set(FLAGS);
    if kind != ShiftKind::Sar {
        code.local_get(RHS)
            .i32_const(1)
            .i32_eq()
            .if_(BlockType::Empty)
            .local_get(FLAGS);
        if kind == ShiftKind::Shl {
            code.local_get(RESULT)
                .i32_const(31)
                .i32_shr_u()
                .local_get(FLAGS)
                .i32_const(1)
                .i32_and()
                .i32_xor();
        } else {
            code.local_get(LHS).i32_const(31).i32_shr_u();
        }
        code.i32_const(11).i32_shl().i32_or().local_set(FLAGS).end();
    }
    code.local_get(RESULT)
        .local_set(register(destination))
        .end();
}

fn arithmetic_flags(code: &mut InstructionSink<'_>, kind: BinaryKind, carry: CarryFlag) {
    code.local_get(FLAGS)
        .i32_const(if matches!(carry, CarryFlag::Preserve) {
            0x401
        } else {
            0x400
        })
        .i32_and()
        .i32_const(2)
        .i32_or();
    if matches!(carry, CarryFlag::Calculate) {
        if matches!(kind, BinaryKind::Add | BinaryKind::Adc) {
            code.local_get(RESULT).local_get(LHS).i32_lt_u();
        } else {
            code.local_get(LHS).local_get(RHS).i32_lt_u();
        }
        if matches!(kind, BinaryKind::Adc | BinaryKind::Sbb) {
            code.local_get(FLAGS).i32_const(1).i32_and();
            if kind == BinaryKind::Adc {
                code.local_get(RESULT).local_get(LHS).i32_eq();
            } else {
                code.local_get(LHS).local_get(RHS).i32_eq();
            }
            code.i32_and().i32_or();
        }
        code.i32_or();
    }
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
    if matches!(kind, BinaryKind::Add | BinaryKind::Adc) {
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
