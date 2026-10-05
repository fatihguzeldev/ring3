use wasm_encoder::{BlockType, InstructionSink, ValType};

use super::{control, locals::*, memory};
use crate::cpu::x86::{
    Register32,
    decode::DecodedInstruction,
    ir::{
        BinaryKind, BranchTarget, ByteArithmeticKind, ByteLogicalKind, BytePredicateKind,
        ByteReadArithmeticKind, ByteRegister, ByteValue, EffectiveAddress, ExtensionKind,
        Location32, MemoryByteArithmeticKind, Operation, ShiftCount, ShiftKind, SmallSource,
        SmallWidth, UnaryKind, Value32,
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
        Operation::MoveByte {
            destination,
            source,
        } => {
            byte_value(code, source);
            insert_byte(code, destination);
        }
        Operation::SetByte {
            condition,
            destination,
        } => {
            control::condition(code, condition);
            insert_byte(code, destination);
        }
        Operation::MemorySetByte { condition, address } => {
            control::condition(code, condition);
            code.local_set(RESULT);
            memory::store_byte_result(code, address, imports, exit_depth);
            store = true;
        }
        Operation::CompareByte { left, right } => {
            byte_value(code, ByteValue::Register(left));
            code.local_set(LHS);
            byte_value(code, right);
            code.local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_sub()
                .i32_const(0xff)
                .i32_and()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 7);
        }
        Operation::ReadCompareByte { left, address } => {
            memory::load_narrow_value(code, address, SmallWidth::Byte, imports, exit_depth);
            code.local_set(RHS);
            byte_value(code, ByteValue::Register(left));
            code.local_set(LHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_sub()
                .i32_const(0xff)
                .i32_and()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 7);
        }
        Operation::TestByte { left, right } => {
            byte_value(code, ByteValue::Register(left));
            byte_value(code, right);
            code.i32_and().local_set(RESULT);
            logical_flags(code, 7);
        }
        Operation::MemoryPredicateByte {
            kind,
            address,
            right,
        } => {
            memory::load_narrow_value(code, address, SmallWidth::Byte, imports, exit_depth);
            code.local_set(LHS);
            byte_value(code, right);
            code.local_set(RHS).local_get(LHS).local_get(RHS);
            match kind {
                BytePredicateKind::Cmp => {
                    code.i32_sub().i32_const(0xff).i32_and().local_set(RESULT);
                    arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 7);
                }
                BytePredicateKind::Test => {
                    code.i32_and().local_set(RESULT);
                    logical_flags(code, 7);
                }
            }
        }
        Operation::MemoryArithmeticByte {
            kind,
            address,
            source,
        } => {
            arithmetic_memory_byte(code, kind, address, source, imports, exit_depth);
            store = true;
        }
        Operation::MemoryLogicalByte {
            kind,
            address,
            source,
        } => {
            logical_memory_byte(code, kind, address, source, imports, exit_depth);
            store = true;
        }
        Operation::ReadLogicalByte {
            kind,
            destination,
            address,
        } => {
            memory::load_narrow_value(code, address, SmallWidth::Byte, imports, exit_depth);
            byte_value(code, ByteValue::Register(destination));
            match kind {
                ByteLogicalKind::And => {
                    code.i32_and();
                }
                ByteLogicalKind::Or => {
                    code.i32_or();
                }
                ByteLogicalKind::Xor => {
                    code.i32_xor();
                }
            }
            code.local_set(RESULT);
            logical_flags(code, 7);
            code.local_get(RESULT);
            insert_byte(code, destination);
        }
        Operation::LogicalByte {
            kind,
            destination,
            source,
        } => {
            byte_value(code, ByteValue::Register(destination));
            byte_value(code, source);
            match kind {
                ByteLogicalKind::And => {
                    code.i32_and();
                }
                ByteLogicalKind::Or => {
                    code.i32_or();
                }
                ByteLogicalKind::Xor => {
                    code.i32_xor();
                }
            }
            code.local_set(RESULT);
            logical_flags(code, 7);
            code.local_get(RESULT);
            insert_byte(code, destination);
        }
        Operation::ArithmeticByte {
            kind,
            destination,
            source,
        } => {
            byte_value(code, ByteValue::Register(destination));
            code.local_set(LHS);
            byte_value(code, source);
            code.local_set(RHS).local_get(LHS).local_get(RHS);
            let binary = match kind {
                ByteArithmeticKind::Add => {
                    code.i32_add();
                    BinaryKind::Add
                }
                ByteArithmeticKind::Adc => {
                    code.i32_add();
                    BinaryKind::Adc
                }
                ByteArithmeticKind::Sub => {
                    code.i32_sub();
                    BinaryKind::Sub
                }
                ByteArithmeticKind::Sbb => {
                    code.i32_sub();
                    BinaryKind::Sbb
                }
            };
            if matches!(binary, BinaryKind::Adc | BinaryKind::Sbb) {
                code.local_get(FLAGS).i32_const(1).i32_and();
                if binary == BinaryKind::Adc {
                    code.i32_add();
                } else {
                    code.i32_sub();
                }
            }
            code.i32_const(0xff).i32_and().local_set(RESULT);
            arithmetic_flags(code, binary, CarryFlag::Calculate, 7);
            code.local_get(RESULT);
            insert_byte(code, destination);
        }
        Operation::ReadArithmeticByte {
            kind,
            destination,
            address,
        } => {
            memory::load_narrow_value(code, address, SmallWidth::Byte, imports, exit_depth);
            // helper validation uses operand scratch; capture operands after it succeeds.
            code.local_set(RHS);
            byte_value(code, ByteValue::Register(destination));
            code.local_set(LHS).local_get(LHS).local_get(RHS);
            let binary = match kind {
                ByteReadArithmeticKind::Add => {
                    code.i32_add();
                    BinaryKind::Add
                }
                ByteReadArithmeticKind::Adc => {
                    code.i32_add();
                    BinaryKind::Adc
                }
                ByteReadArithmeticKind::Sub => {
                    code.i32_sub();
                    BinaryKind::Sub
                }
                ByteReadArithmeticKind::Sbb => {
                    code.i32_sub();
                    BinaryKind::Sbb
                }
            };
            if matches!(binary, BinaryKind::Adc | BinaryKind::Sbb) {
                code.local_get(FLAGS).i32_const(1).i32_and();
                if binary == BinaryKind::Adc {
                    code.i32_add();
                } else {
                    code.i32_sub();
                }
            }
            code.i32_const(0xff).i32_and().local_set(RESULT);
            arithmetic_flags(code, binary, CarryFlag::Calculate, 7);
            code.local_get(RESULT);
            insert_byte(code, destination);
        }
        Operation::LoadByte {
            destination,
            address,
        } => {
            memory::load_narrow_value(code, address, SmallWidth::Byte, imports, exit_depth);
            let (parent, high) = match destination {
                ByteRegister::Al => (Register32::Eax, false),
                ByteRegister::Cl => (Register32::Ecx, false),
                ByteRegister::Dl => (Register32::Edx, false),
                ByteRegister::Bl => (Register32::Ebx, false),
                ByteRegister::Ah => (Register32::Eax, true),
                ByteRegister::Ch => (Register32::Ecx, true),
                ByteRegister::Dh => (Register32::Edx, true),
                ByteRegister::Bh => (Register32::Ebx, true),
            };
            if high {
                code.i32_const(8).i32_shl();
            }
            code.local_get(register(parent))
                .i32_const(if high { !0xff00 } else { !0xff })
                .i32_and()
                .i32_or()
                .local_set(register(parent));
        }
        Operation::StoreByte { address, source } => {
            memory::store_byte(code, address, source, imports, exit_depth);
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
                | BinaryKind::Cmp => arithmetic_flags(code, kind, CarryFlag::Calculate, 31),
                BinaryKind::And | BinaryKind::Or | BinaryKind::Xor | BinaryKind::Test => {
                    logical_flags(code, 31)
                }
            }
            if writes && let Location32::Register(destination) = destination {
                code.local_get(RESULT).local_set(register(destination));
            }
        }
        Operation::UnaryByte { kind, destination } => unary_byte(code, kind, destination),
        Operation::MemoryUnaryByte { kind, address } => {
            unary_memory_byte(code, kind, address, imports, exit_depth);
            store = true;
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
        Operation::ShiftByte {
            kind,
            destination,
            count,
        } => shift_byte(code, kind, destination, count),
        Operation::MemoryShiftByte { kind, address } => {
            shift_memory_byte(code, kind, address, imports, exit_depth);
            store = true;
        }
        Operation::Shift {
            kind,
            destination: Location32::Register(destination),
            count,
        } => shift(code, kind, destination, count),
        Operation::Shift {
            kind,
            destination: Location32::Memory(address),
            count,
        } => {
            shift_memory(code, kind, address, count, imports, exit_depth);
            store = true;
        }
        Operation::SignedMultiply {
            destination,
            source,
            immediate,
        } => {
            signed_multiply(code, destination, source, immediate, imports, exit_depth);
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

fn unary_memory_byte(
    code: &mut InstructionSink<'_>,
    kind: UnaryKind,
    address: EffectiveAddress,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::load_narrow_value(code, address, SmallWidth::Byte, imports, exit_depth);
    code.local_set(RESULT);
    match kind {
        UnaryKind::Inc => {
            code.local_get(RESULT).i32_const(1).i32_add();
        }
        UnaryKind::Dec => {
            code.local_get(RESULT).i32_const(1).i32_sub();
        }
        UnaryKind::Neg => {
            code.i32_const(0).local_get(RESULT).i32_sub();
        }
        UnaryKind::Not => {
            code.local_get(RESULT).i32_const(0xff).i32_xor();
        }
    }
    code.i32_const(0xff).i32_and().local_set(RESULT);
    memory::store_byte_result(code, address, imports, exit_depth);
    // store validation uses LHS/RHS scratch; recover operands only after success.
    let (binary, carry) = match kind {
        UnaryKind::Inc => {
            code.local_get(RESULT)
                .i32_const(1)
                .i32_sub()
                .i32_const(0xff)
                .i32_and()
                .local_set(LHS)
                .i32_const(1)
                .local_set(RHS);
            (BinaryKind::Add, CarryFlag::Preserve)
        }
        UnaryKind::Dec => {
            code.local_get(RESULT)
                .i32_const(1)
                .i32_add()
                .i32_const(0xff)
                .i32_and()
                .local_set(LHS)
                .i32_const(1)
                .local_set(RHS);
            (BinaryKind::Sub, CarryFlag::Preserve)
        }
        UnaryKind::Neg => {
            code.i32_const(0)
                .local_set(LHS)
                .i32_const(0)
                .local_get(RESULT)
                .i32_sub()
                .i32_const(0xff)
                .i32_and()
                .local_set(RHS);
            (BinaryKind::Sub, CarryFlag::Calculate)
        }
        UnaryKind::Not => return,
    };
    arithmetic_flags(code, binary, carry, 7);
}

fn logical_memory_byte(
    code: &mut InstructionSink<'_>,
    kind: ByteLogicalKind,
    address: EffectiveAddress,
    source: ByteValue,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::load_narrow_value(code, address, SmallWidth::Byte, imports, exit_depth);
    byte_value(code, source);
    match kind {
        ByteLogicalKind::And => {
            code.i32_and();
        }
        ByteLogicalKind::Or => {
            code.i32_or();
        }
        ByteLogicalKind::Xor => {
            code.i32_xor();
        }
    }
    code.local_set(RESULT);
    memory::store_byte_result(code, address, imports, exit_depth);
    logical_flags(code, 7);
}

fn arithmetic_memory_byte(
    code: &mut InstructionSink<'_>,
    kind: MemoryByteArithmeticKind,
    address: EffectiveAddress,
    source: ByteValue,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::load_narrow_value(code, address, SmallWidth::Byte, imports, exit_depth);
    byte_value(code, source);
    let binary = match kind {
        MemoryByteArithmeticKind::Add => {
            code.i32_add();
            BinaryKind::Add
        }
        MemoryByteArithmeticKind::Adc => {
            code.i32_add();
            BinaryKind::Adc
        }
        MemoryByteArithmeticKind::Sub => {
            code.i32_sub();
            BinaryKind::Sub
        }
        MemoryByteArithmeticKind::Sbb => {
            code.i32_sub();
            BinaryKind::Sbb
        }
    };
    if matches!(binary, BinaryKind::Adc | BinaryKind::Sbb) {
        code.local_get(FLAGS).i32_const(1).i32_and();
        if binary == BinaryKind::Adc {
            code.i32_add();
        } else {
            code.i32_sub();
        }
    }
    code.i32_const(0xff).i32_and().local_set(RESULT);
    memory::store_byte_result(code, address, imports, exit_depth);
    // store validation uses operand scratch; recover the original byte after success.
    byte_value(code, source);
    code.local_set(RHS).local_get(RESULT).local_get(RHS);
    if matches!(
        kind,
        MemoryByteArithmeticKind::Add | MemoryByteArithmeticKind::Adc
    ) {
        code.i32_sub();
    } else {
        code.i32_add();
    }
    if matches!(binary, BinaryKind::Adc | BinaryKind::Sbb) {
        code.local_get(FLAGS).i32_const(1).i32_and();
        if binary == BinaryKind::Adc {
            code.i32_sub();
        } else {
            code.i32_add();
        }
    }
    code.i32_const(0xff).i32_and().local_set(LHS);
    arithmetic_flags(code, binary, CarryFlag::Calculate, 7);
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

fn byte_parent(byte: ByteRegister) -> (Register32, bool) {
    match byte {
        ByteRegister::Al => (Register32::Eax, false),
        ByteRegister::Cl => (Register32::Ecx, false),
        ByteRegister::Dl => (Register32::Edx, false),
        ByteRegister::Bl => (Register32::Ebx, false),
        ByteRegister::Ah => (Register32::Eax, true),
        ByteRegister::Ch => (Register32::Ecx, true),
        ByteRegister::Dh => (Register32::Edx, true),
        ByteRegister::Bh => (Register32::Ebx, true),
    }
}

fn insert_byte(code: &mut InstructionSink<'_>, destination: ByteRegister) {
    let (parent, high) = byte_parent(destination);
    if high {
        code.i32_const(8).i32_shl();
    }
    code.local_get(register(parent))
        .i32_const(if high { !0xff00 } else { !0xff })
        .i32_and()
        .i32_or()
        .local_set(register(parent));
}

fn byte_value(code: &mut InstructionSink<'_>, value: ByteValue) {
    match value {
        ByteValue::Immediate(value) => {
            code.i32_const(i32::from(value));
        }
        ByteValue::Register(source) => {
            let (parent, high) = byte_parent(source);
            code.local_get(register(parent));
            if high {
                code.i32_const(8).i32_shr_u();
            }
            code.i32_const(0xff).i32_and();
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
    arithmetic_flags(code, binary, carry, 31);
}

fn unary_byte(code: &mut InstructionSink<'_>, kind: UnaryKind, destination: ByteRegister) {
    byte_value(code, ByteValue::Register(destination));
    code.local_set(LHS);
    let (binary, carry) = match kind {
        UnaryKind::Not => {
            code.local_get(LHS).i32_const(0xff).i32_xor();
            insert_byte(code, destination);
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
    code.i32_const(0xff).i32_and().local_set(RESULT);
    arithmetic_flags(code, binary, carry, 7);
    code.local_get(RESULT);
    insert_byte(code, destination);
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
    arithmetic_flags(code, binary, carry, 31);
    code.local_get(RESULT).local_set(register(destination));
}

fn shift_byte(
    code: &mut InstructionSink<'_>,
    kind: ShiftKind,
    destination: ByteRegister,
    count: ShiftCount,
) {
    byte_value(code, ByteValue::Register(destination));
    code.local_set(LHS);
    shift_count(code, count);
    code.local_tee(RHS).if_(BlockType::Empty);
    shift_byte_value(code, kind);
    shift_byte_flags(code, kind);
    code.local_get(RESULT);
    insert_byte(code, destination);
    code.end();
}

fn shift_memory_byte(
    code: &mut InstructionSink<'_>,
    kind: ShiftKind,
    address: EffectiveAddress,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::load_narrow_value(code, address, SmallWidth::Byte, imports, exit_depth);
    code.local_set(LHS).i32_const(1).local_set(RHS);
    shift_byte_value(code, kind);
    // checked store clobbers operand scratch; preserve the lossy original byte.
    code.local_get(LHS);
    memory::store_byte_result(code, address, imports, exit_depth);
    code.local_set(LHS).i32_const(1).local_set(RHS);
    shift_byte_flags(code, kind);
}

fn shift_byte_value(code: &mut InstructionSink<'_>, kind: ShiftKind) {
    code.local_get(LHS);
    if kind == ShiftKind::Sar {
        code.i32_const(24).i32_shl().i32_const(24).i32_shr_s();
    }
    code.local_get(RHS);
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
    code.i32_const(0xff).i32_and().local_set(RESULT);
}

fn shift_byte_flags(code: &mut InstructionSink<'_>, kind: ShiftKind) {
    // undefined af, multi-bit of and shl/shr carry at counts >= 8 are cleared by this profile.
    logical_flags(code, 7);
    code.local_get(RHS)
        .i32_const(8)
        .i32_lt_u()
        .if_(BlockType::Empty)
        .local_get(FLAGS)
        .local_get(LHS);
    if kind == ShiftKind::Shl {
        code.i32_const(8).local_get(RHS).i32_sub();
    } else {
        code.local_get(RHS).i32_const(1).i32_sub();
    }
    code.i32_shr_u()
        .i32_const(1)
        .i32_and()
        .i32_or()
        .local_set(FLAGS);
    if kind == ShiftKind::Sar {
        code.else_()
            .local_get(FLAGS)
            .local_get(LHS)
            .i32_const(7)
            .i32_shr_u()
            .i32_or()
            .local_set(FLAGS);
    }
    code.end();
    if kind != ShiftKind::Sar {
        code.local_get(RHS)
            .i32_const(1)
            .i32_eq()
            .if_(BlockType::Empty)
            .local_get(FLAGS);
        if kind == ShiftKind::Shl {
            code.local_get(RESULT)
                .i32_const(7)
                .i32_shr_u()
                .local_get(FLAGS)
                .i32_const(1)
                .i32_and()
                .i32_xor();
        } else {
            code.local_get(LHS).i32_const(7).i32_shr_u();
        }
        code.i32_const(11).i32_shl().i32_or().local_set(FLAGS).end();
    }
}

fn shift(
    code: &mut InstructionSink<'_>,
    kind: ShiftKind,
    destination: Register32,
    count: ShiftCount,
) {
    code.local_get(register(destination)).local_set(LHS);
    shift_count(code, count);
    code.local_tee(RHS).if_(BlockType::Empty);
    shift_value(code, kind);
    shift_flags(code, kind);
    code.local_get(RESULT)
        .local_set(register(destination))
        .end();
}

fn shift_memory(
    code: &mut InstructionSink<'_>,
    kind: ShiftKind,
    address: EffectiveAddress,
    count: ShiftCount,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::load_result(code, address, imports, exit_depth);
    code.local_get(RESULT).local_set(LHS);
    shift_count(code, count);
    code.local_set(RHS);
    shift_value(code, kind);
    // store validation clobbers operand scratch; keep the lossy inputs on the operand stack.
    code.local_get(LHS).local_get(RHS);
    memory::store_result(code, address, imports, exit_depth);
    code.local_set(RHS)
        .local_set(LHS)
        .local_get(RHS)
        .if_(BlockType::Empty);
    shift_flags(code, kind);
    code.end();
}

fn shift_count(code: &mut InstructionSink<'_>, count: ShiftCount) {
    match count {
        ShiftCount::Immediate(count) => {
            code.i32_const(i32::from(count));
        }
        ShiftCount::Cl => {
            code.local_get(register(Register32::Ecx));
        }
    }
    code.i32_const(31).i32_and();
}

fn shift_value(code: &mut InstructionSink<'_>, kind: ShiftKind) {
    code.local_get(LHS).local_get(RHS);
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
}

fn shift_flags(code: &mut InstructionSink<'_>, kind: ShiftKind) {
    // af and multi-bit of are undefined; this profile clears them.
    logical_flags(code, 31);
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
}

fn signed_multiply(
    code: &mut InstructionSink<'_>,
    destination: Register32,
    source: Location32,
    immediate: Option<u32>,
    imports: memory::Imports,
    exit_depth: u32,
) {
    let source = match source {
        Location32::Register(source) => register(source),
        Location32::Memory(address) => {
            // helper validation uses operand scratch; capture operands after it succeeds.
            memory::load_result(code, address, imports, exit_depth);
            RESULT
        }
    };
    match immediate {
        Some(immediate) => {
            code.local_get(source)
                .local_set(LHS)
                .i32_const(immediate as i32)
                .local_set(RHS);
        }
        None => {
            code.local_get(register(destination))
                .local_set(LHS)
                .local_get(source)
                .local_set(RHS);
        }
    }
    code.local_get(LHS)
        .local_get(RHS)
        .i32_mul()
        .local_set(RESULT);
    // PF/AF/ZF/SF are undefined; this profile clears them.
    code.local_get(FLAGS)
        .i32_const(0x402)
        .i32_and()
        .local_get(LHS)
        .i64_extend_i32_s()
        .local_get(RHS)
        .i64_extend_i32_s()
        .i64_mul()
        .local_get(RESULT)
        .i64_extend_i32_s()
        .i64_ne()
        .i32_const(0x801)
        .i32_mul()
        .i32_or()
        .local_set(FLAGS)
        .local_get(RESULT)
        .local_set(register(destination));
}

fn arithmetic_flags(
    code: &mut InstructionSink<'_>,
    kind: BinaryKind,
    carry: CarryFlag,
    sign_bit: i32,
) {
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
    zero_sign_flags(code, sign_bit);
    code.local_get(LHS).local_get(RHS).i32_xor();
    if matches!(kind, BinaryKind::Add | BinaryKind::Adc) {
        code.i32_const(-1).i32_xor();
    }
    code.local_get(LHS)
        .local_get(RESULT)
        .i32_xor()
        .i32_and()
        .i32_const(sign_bit)
        .i32_shr_u()
        .i32_const(11)
        .i32_shl()
        .i32_or()
        .local_set(FLAGS);
}

fn logical_flags(code: &mut InstructionSink<'_>, sign_bit: i32) {
    // af is undefined on x86; this profile deterministically clears it with cf/of.
    code.local_get(FLAGS)
        .i32_const(0x400)
        .i32_and()
        .i32_const(2)
        .i32_or();
    parity_flag(code);
    zero_sign_flags(code, sign_bit);
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

fn zero_sign_flags(code: &mut InstructionSink<'_>, sign_bit: i32) {
    code.local_get(RESULT)
        .i32_eqz()
        .i32_const(6)
        .i32_shl()
        .i32_or()
        .local_get(RESULT)
        .i32_const(sign_bit - 7)
        .i32_shr_u()
        .i32_const(0x80)
        .i32_and()
        .i32_or();
}
