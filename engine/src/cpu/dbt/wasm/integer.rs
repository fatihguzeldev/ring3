use wasm_encoder::{BlockType, InstructionSink, ValType};

use super::{control, locals::*, memory, x87};
use crate::cpu::x86::{
    Register32,
    decode::DecodedInstruction,
    ir::{
        BinaryKind, BitIndex, BitScanKind, BitTestKind, BranchTarget, ByteArithmeticKind,
        ByteLogicalKind, BytePredicateKind, ByteReadArithmeticKind, ByteRegister, ByteValue,
        CarryKind, Condition, CountBranchKind, DivideKind, DoubleShiftKind, EffectiveAddress,
        ExtensionKind, Location32, MemoryByteArithmeticKind, MultiplyKind, Operation, RotateKind,
        ShiftCount, ShiftKind, SmallSource, SmallWidth, UnaryKind, Value32, WordArithmeticKind,
        WordLogicalKind, WordMemoryArithmeticKind, WordReadArithmeticKind, WordValue,
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
        Operation::InitializeX87 => x87::initialize(code),
        Operation::ClearX87Exceptions => x87::clear_exceptions(code),
        Operation::X87StatusToAx => x87::status_to_ax(code),
        Operation::X87StatusToMemory { address } => {
            x87::status_value(code);
            code.local_set(RESULT);
            memory::store_word_result(code, address, imports, exit_depth);
            store = true;
        }
        Operation::X87ControlToMemory { address } => {
            x87::control_value(code);
            code.local_set(RESULT);
            memory::store_word_result(code, address, imports, exit_depth);
            store = true;
        }
        Operation::SignExtendHigh => {
            code.local_get(register(Register32::Eax))
                .i32_const(31)
                .i32_shr_s()
                .local_set(register(Register32::Edx));
        }
        Operation::FlagsToAh => {
            code.local_get(FLAGS)
                .i32_const(0xd5)
                .i32_and()
                .i32_const(2)
                .i32_or();
            insert_byte(code, ByteRegister::Ah);
        }
        Operation::AhToFlags => {
            code.local_get(FLAGS).i32_const(!0xd5).i32_and();
            byte_value(code, ByteValue::Register(ByteRegister::Ah));
            code.i32_const(0xd5).i32_and().i32_or().local_set(FLAGS);
        }
        Operation::Carry { kind } => {
            code.local_get(FLAGS);
            match kind {
                CarryKind::Clear => {
                    code.i32_const(!1).i32_and();
                }
                CarryKind::Set => {
                    code.i32_const(1).i32_or();
                }
                CarryKind::Complement => {
                    code.i32_const(1).i32_xor();
                }
            }
            code.local_set(FLAGS);
        }
        Operation::Direction { set } => {
            code.local_get(FLAGS);
            if set {
                code.i32_const(0x400).i32_or();
            } else {
                code.i32_const(!0x400).i32_and();
            }
            code.local_set(FLAGS);
        }
        Operation::MoveStringByte => {
            memory::load_narrow_value(
                code,
                EffectiveAddress {
                    base: Some(Register32::Esi),
                    index: None,
                    scale: 1,
                    displacement: 0,
                },
                SmallWidth::Byte,
                imports,
                exit_depth,
            );
            code.local_set(RESULT);
            memory::store_byte_result(
                code,
                EffectiveAddress {
                    base: Some(Register32::Edi),
                    index: None,
                    scale: 1,
                    displacement: 0,
                },
                imports,
                exit_depth,
            );
            code.i32_const(-1)
                .i32_const(1)
                .local_get(FLAGS)
                .i32_const(0x400)
                .i32_and()
                .select()
                .local_set(RESULT);
            for pointer in [Register32::Esi, Register32::Edi] {
                code.local_get(register(pointer))
                    .local_get(RESULT)
                    .i32_add()
                    .local_set(register(pointer));
            }
            store = true;
        }
        Operation::MoveStringWord => {
            memory::load_narrow_value(
                code,
                EffectiveAddress {
                    base: Some(Register32::Esi),
                    index: None,
                    scale: 1,
                    displacement: 0,
                },
                SmallWidth::Word,
                imports,
                exit_depth,
            );
            code.local_set(RESULT);
            memory::store_word_result(
                code,
                EffectiveAddress {
                    base: Some(Register32::Edi),
                    index: None,
                    scale: 1,
                    displacement: 0,
                },
                imports,
                exit_depth,
            );
            word_string_pointer(code, Register32::Esi);
            word_string_pointer(code, Register32::Edi);
            store = true;
        }
        Operation::MoveStringDword => {
            memory::load_result(
                code,
                EffectiveAddress {
                    base: Some(Register32::Esi),
                    index: None,
                    scale: 1,
                    displacement: 0,
                },
                imports,
                exit_depth,
            );
            memory::store_result(
                code,
                EffectiveAddress {
                    base: Some(Register32::Edi),
                    index: None,
                    scale: 1,
                    displacement: 0,
                },
                imports,
                exit_depth,
            );
            code.i32_const(-4)
                .i32_const(4)
                .local_get(FLAGS)
                .i32_const(0x400)
                .i32_and()
                .select()
                .local_set(RESULT);
            for pointer in [Register32::Esi, Register32::Edi] {
                code.local_get(register(pointer))
                    .local_get(RESULT)
                    .i32_add()
                    .local_set(register(pointer));
            }
            store = true;
        }
        Operation::LoadStringByte => {
            memory::load_narrow_value(
                code,
                EffectiveAddress {
                    base: Some(Register32::Esi),
                    index: None,
                    scale: 1,
                    displacement: 0,
                },
                SmallWidth::Byte,
                imports,
                exit_depth,
            );
            insert_byte(code, ByteRegister::Al);
            string_pointer(code, Register32::Esi);
        }
        Operation::LoadStringWord => {
            memory::load_narrow_value(
                code,
                EffectiveAddress {
                    base: Some(Register32::Esi),
                    index: None,
                    scale: 1,
                    displacement: 0,
                },
                SmallWidth::Word,
                imports,
                exit_depth,
            );
            code.local_get(register(Register32::Eax))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(Register32::Eax));
            word_string_pointer(code, Register32::Esi);
        }
        Operation::LoadStringDword => {
            memory::load_result(
                code,
                EffectiveAddress {
                    base: Some(Register32::Esi),
                    index: None,
                    scale: 1,
                    displacement: 0,
                },
                imports,
                exit_depth,
            );
            code.local_get(RESULT).local_set(register(Register32::Eax));
            dword_string_pointer(code, Register32::Esi);
        }
        Operation::TranslateByte => {
            code.local_get(register(Register32::Ebx))
                .local_get(register(Register32::Eax))
                .i32_const(0xff)
                .i32_and()
                .i32_add()
                .local_set(ADDRESS);
            memory::load_narrow_at_address(code, SmallWidth::Byte, imports, exit_depth);
            insert_byte(code, ByteRegister::Al);
        }
        Operation::StoreStringByte => {
            byte_value(code, ByteValue::Register(ByteRegister::Al));
            code.local_set(RESULT);
            memory::store_byte_result(
                code,
                EffectiveAddress {
                    base: Some(Register32::Edi),
                    index: None,
                    scale: 1,
                    displacement: 0,
                },
                imports,
                exit_depth,
            );
            string_pointer(code, Register32::Edi);
            store = true;
        }
        Operation::StoreStringDword => {
            code.local_get(register(Register32::Eax)).local_set(RESULT);
            memory::store_result(
                code,
                EffectiveAddress {
                    base: Some(Register32::Edi),
                    index: None,
                    scale: 1,
                    displacement: 0,
                },
                imports,
                exit_depth,
            );
            dword_string_pointer(code, Register32::Edi);
            store = true;
        }
        Operation::StoreStringWord => {
            code.local_get(register(Register32::Eax))
                .i32_const(0xffff)
                .i32_and()
                .local_set(RESULT);
            memory::store_word_result(
                code,
                EffectiveAddress {
                    base: Some(Register32::Edi),
                    index: None,
                    scale: 1,
                    displacement: 0,
                },
                imports,
                exit_depth,
            );
            word_string_pointer(code, Register32::Edi);
            store = true;
        }
        Operation::CompareStringByte => {
            memory::load_narrow_value(
                code,
                EffectiveAddress {
                    base: Some(Register32::Esi),
                    index: None,
                    scale: 1,
                    displacement: 0,
                },
                SmallWidth::Byte,
                imports,
                exit_depth,
            );
            code.local_set(RESULT);
            memory::load_narrow_value(
                code,
                EffectiveAddress {
                    base: Some(Register32::Edi),
                    index: None,
                    scale: 1,
                    displacement: 0,
                },
                SmallWidth::Byte,
                imports,
                exit_depth,
            );
            // the second checked read clobbers lhs/rhs; result retains the first byte.
            code.local_set(RHS)
                .local_get(RESULT)
                .local_set(LHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_sub()
                .i32_const(0xff)
                .i32_and()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 7);
            string_pointer(code, Register32::Esi);
            string_pointer(code, Register32::Edi);
        }
        Operation::ScanStringByte => {
            memory::load_narrow_value(
                code,
                EffectiveAddress {
                    base: Some(Register32::Edi),
                    index: None,
                    scale: 1,
                    displacement: 0,
                },
                SmallWidth::Byte,
                imports,
                exit_depth,
            );
            code.local_set(RHS);
            byte_value(code, ByteValue::Register(ByteRegister::Al));
            code.local_set(LHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_sub()
                .i32_const(0xff)
                .i32_and()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 7);
            string_pointer(code, Register32::Edi);
        }
        Operation::CompareStringWord => {
            memory::load_narrow_value(
                code,
                EffectiveAddress {
                    base: Some(Register32::Esi),
                    index: None,
                    scale: 1,
                    displacement: 0,
                },
                SmallWidth::Word,
                imports,
                exit_depth,
            );
            code.local_set(RESULT);
            memory::load_narrow_value(
                code,
                EffectiveAddress {
                    base: Some(Register32::Edi),
                    index: None,
                    scale: 1,
                    displacement: 0,
                },
                SmallWidth::Word,
                imports,
                exit_depth,
            );
            // the second checked read clobbers lhs/rhs; result retains the first word.
            code.local_set(RHS)
                .local_get(RESULT)
                .local_set(LHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_sub()
                .i32_const(0xffff)
                .i32_and()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 15);
            word_string_pointer(code, Register32::Esi);
            word_string_pointer(code, Register32::Edi);
        }
        Operation::ScanStringWord => {
            memory::load_narrow_value(
                code,
                EffectiveAddress {
                    base: Some(Register32::Edi),
                    index: None,
                    scale: 1,
                    displacement: 0,
                },
                SmallWidth::Word,
                imports,
                exit_depth,
            );
            code.local_set(RHS)
                .local_get(register(Register32::Eax))
                .i32_const(0xffff)
                .i32_and()
                .local_set(LHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_sub()
                .i32_const(0xffff)
                .i32_and()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 15);
            word_string_pointer(code, Register32::Edi);
        }
        Operation::CompareStringDword => {
            memory::load_result(
                code,
                EffectiveAddress {
                    base: Some(Register32::Esi),
                    index: None,
                    scale: 1,
                    displacement: 0,
                },
                imports,
                exit_depth,
            );
            // the second checked read overwrites result and scratch; retain the first word on the stack.
            code.local_get(RESULT);
            memory::load_result(
                code,
                EffectiveAddress {
                    base: Some(Register32::Edi),
                    index: None,
                    scale: 1,
                    displacement: 0,
                },
                imports,
                exit_depth,
            );
            code.local_get(RESULT)
                .local_set(RHS)
                .local_set(LHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_sub()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 31);
            dword_string_pointer(code, Register32::Esi);
            dword_string_pointer(code, Register32::Edi);
        }
        Operation::ScanStringDword => {
            memory::load_result(
                code,
                EffectiveAddress {
                    base: Some(Register32::Edi),
                    index: None,
                    scale: 1,
                    displacement: 0,
                },
                imports,
                exit_depth,
            );
            code.local_get(RESULT)
                .local_set(RHS)
                .local_get(register(Register32::Eax))
                .local_set(LHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_sub()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 31);
            dword_string_pointer(code, Register32::Edi);
        }
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
        Operation::MoveWord {
            destination,
            source,
        } => {
            word_value(code, source);
            code.local_get(register(destination))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(destination));
        }
        Operation::ArithmeticWord {
            kind,
            destination,
            source,
        } => {
            word_value(code, WordValue::Register(destination));
            code.local_set(LHS);
            word_value(code, source);
            code.local_set(RHS).local_get(LHS).local_get(RHS);
            let binary = match kind {
                WordArithmeticKind::Add => {
                    code.i32_add();
                    BinaryKind::Add
                }
                WordArithmeticKind::Sub => {
                    code.i32_sub();
                    BinaryKind::Sub
                }
                WordArithmeticKind::Adc => {
                    code.i32_add();
                    BinaryKind::Adc
                }
                WordArithmeticKind::Sbb => {
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
            code.i32_const(0xffff).i32_and().local_set(RESULT);
            arithmetic_flags(code, binary, CarryFlag::Calculate, 15);
            code.local_get(RESULT)
                .local_get(register(destination))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(destination));
        }
        Operation::ReadArithmeticWord {
            kind,
            destination,
            address,
        } => {
            memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
            // helper validation uses operand scratch; capture operands after it succeeds.
            code.local_set(RHS);
            word_value(code, WordValue::Register(destination));
            code.local_set(LHS).local_get(LHS).local_get(RHS);
            let binary = match kind {
                WordReadArithmeticKind::Add => {
                    code.i32_add();
                    BinaryKind::Add
                }
                WordReadArithmeticKind::Sub => {
                    code.i32_sub();
                    BinaryKind::Sub
                }
                WordReadArithmeticKind::Adc => {
                    code.i32_add();
                    BinaryKind::Adc
                }
                WordReadArithmeticKind::Sbb => {
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
            code.i32_const(0xffff).i32_and().local_set(RESULT);
            arithmetic_flags(code, binary, CarryFlag::Calculate, 15);
            code.local_get(RESULT)
                .local_get(register(destination))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(destination));
        }
        Operation::MemoryArithmeticWord {
            kind,
            address,
            source,
        } => {
            memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
            word_value(code, source);
            let binary = match kind {
                WordMemoryArithmeticKind::Add => {
                    code.i32_add();
                    BinaryKind::Add
                }
                WordMemoryArithmeticKind::Sub => {
                    code.i32_sub();
                    BinaryKind::Sub
                }
                WordMemoryArithmeticKind::Adc => {
                    code.i32_add();
                    BinaryKind::Adc
                }
                WordMemoryArithmeticKind::Sbb => {
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
            code.i32_const(0xffff).i32_and().local_set(RESULT);
            memory::store_word_result(code, address, imports, exit_depth);
            // store validation uses operand scratch; recover the original word after success.
            word_value(code, source);
            code.local_set(RHS).local_get(RESULT).local_get(RHS);
            if matches!(
                kind,
                WordMemoryArithmeticKind::Add | WordMemoryArithmeticKind::Adc
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
            code.i32_const(0xffff).i32_and().local_set(LHS);
            arithmetic_flags(code, binary, CarryFlag::Calculate, 15);
            store = true;
        }
        Operation::MemoryLogicalWord {
            kind,
            address,
            source,
        } => {
            memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
            // helper validation uses operand scratch; capture the source after success.
            word_value(code, WordValue::Register(source));
            match kind {
                WordLogicalKind::And => code.i32_and(),
                WordLogicalKind::Or => code.i32_or(),
                WordLogicalKind::Xor => code.i32_xor(),
            };
            code.local_set(RESULT);
            memory::store_word_result(code, address, imports, exit_depth);
            logical_flags(code, 15);
            store = true;
        }
        Operation::ReadLogicalWord {
            kind,
            destination,
            address,
        } => {
            memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
            // helper validation uses operand scratch; capture the destination after success.
            word_value(code, WordValue::Register(destination));
            match kind {
                WordLogicalKind::And => code.i32_and(),
                WordLogicalKind::Or => code.i32_or(),
                WordLogicalKind::Xor => code.i32_xor(),
            };
            code.local_set(RESULT);
            logical_flags(code, 15);
            code.local_get(RESULT)
                .local_get(register(destination))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(destination));
        }
        Operation::LogicalWord {
            kind,
            destination,
            source,
        } => {
            word_value(code, WordValue::Register(destination));
            word_value(code, source);
            match kind {
                WordLogicalKind::And => code.i32_and(),
                WordLogicalKind::Or => code.i32_or(),
                WordLogicalKind::Xor => code.i32_xor(),
            };
            code.local_set(RESULT);
            logical_flags(code, 15);
            code.local_get(RESULT)
                .local_get(register(destination))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(destination));
        }
        Operation::CompareWord { left, right } => {
            word_value(code, WordValue::Register(left));
            code.local_set(LHS);
            word_value(code, right);
            code.local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_sub()
                .i32_const(0xffff)
                .i32_and()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 15);
        }
        Operation::TestWord { left, right } => {
            word_value(code, WordValue::Register(left));
            word_value(code, right);
            code.i32_and().local_set(RESULT);
            logical_flags(code, 15);
        }
        Operation::MemoryCompareWord { address, right } => {
            memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
            code.local_set(LHS);
            word_value(code, right);
            code.local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_sub()
                .i32_const(0xffff)
                .i32_and()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 15);
        }
        Operation::ReadCompareWord { left, address } => {
            memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
            code.local_set(RHS);
            word_value(code, WordValue::Register(left));
            code.local_set(LHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_sub()
                .i32_const(0xffff)
                .i32_and()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 15);
        }
        Operation::MemoryTestWord { address, right } => {
            memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
            word_value(code, right);
            code.i32_and().local_set(RESULT);
            logical_flags(code, 15);
        }
        Operation::SetByte {
            condition,
            destination,
        } => {
            control::condition(code, condition);
            insert_byte(code, destination);
        }
        Operation::ConditionalMove {
            condition,
            destination,
            source,
        } => {
            code.local_get(register(source));
            code.local_get(register(destination));
            control::condition(code, condition);
            code.select();
            code.local_set(register(destination));
        }
        Operation::ConditionalMoveWord {
            condition,
            destination,
            source,
        } => {
            word_value(code, WordValue::Register(source));
            word_value(code, WordValue::Register(destination));
            control::condition(code, condition);
            code.select()
                .local_get(register(destination))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(destination));
        }
        Operation::ReadConditionalMove {
            condition,
            destination,
            address,
        } => {
            memory::load_result(code, address, imports, exit_depth);
            code.local_get(RESULT).local_get(register(destination));
            control::condition(code, condition);
            code.select().local_set(register(destination));
        }
        Operation::ReadConditionalMoveWord {
            condition,
            destination,
            address,
        } => {
            memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
            word_value(code, WordValue::Register(destination));
            control::condition(code, condition);
            code.select()
                .local_get(register(destination))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(destination));
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
        Operation::LoadWord {
            destination,
            address,
        } => {
            memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
            code.local_get(register(destination))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(destination));
        }
        Operation::StoreWord { address, source } => {
            word_value(code, source);
            code.local_set(RESULT);
            memory::store_word_result(code, address, imports, exit_depth);
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
        Operation::ByteSwap { destination } => {
            let destination = register(destination);
            code.local_get(destination)
                .i32_const(24)
                .i32_shl()
                .local_get(destination)
                .i32_const(0xff00)
                .i32_and()
                .i32_const(8)
                .i32_shl()
                .i32_or()
                .local_get(destination)
                .i32_const(8)
                .i32_shr_u()
                .i32_const(0xff00)
                .i32_and()
                .i32_or()
                .local_get(destination)
                .i32_const(24)
                .i32_shr_u()
                .i32_or()
                .local_set(destination);
        }
        Operation::Exchange { left, right } => {
            code.local_get(register(left))
                .local_get(register(right))
                .local_set(register(left))
                .local_set(register(right));
        }
        Operation::ExchangeAdd {
            destination,
            source,
        } => {
            code.local_get(register(destination))
                .local_set(LHS)
                .local_get(register(source))
                .local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_add()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Add, CarryFlag::Calculate, 31);
            code.local_get(LHS)
                .local_set(register(source))
                .local_get(RESULT)
                .local_set(register(destination));
        }
        Operation::ExchangeAddWord {
            destination,
            source,
        } => {
            word_value(code, WordValue::Register(destination));
            code.local_set(LHS);
            word_value(code, WordValue::Register(source));
            code.local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_add()
                .i32_const(0xffff)
                .i32_and()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Add, CarryFlag::Calculate, 15);
            code.local_get(LHS)
                .local_get(register(source))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(source))
                .local_get(RESULT)
                .local_get(register(destination))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(destination));
        }
        Operation::MemoryExchangeAdd { address, source } => {
            memory::load_result(code, address, imports, exit_depth);
            code.local_get(RESULT)
                .local_set(LHS)
                .local_get(register(source))
                .local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_add()
                .local_set(RESULT)
                .local_get(LHS)
                .local_get(RHS);
            memory::store_result(code, address, imports, exit_depth);
            code.local_set(RHS).local_set(LHS);
            arithmetic_flags(code, BinaryKind::Add, CarryFlag::Calculate, 31);
            code.local_get(LHS).local_set(register(source));
            store = true;
        }
        Operation::ExchangeAddByte {
            destination,
            source,
        } => {
            byte_value(code, ByteValue::Register(destination));
            code.local_set(LHS);
            byte_value(code, ByteValue::Register(source));
            code.local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_add()
                .i32_const(0xff)
                .i32_and()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Add, CarryFlag::Calculate, 7);
            code.local_get(LHS);
            insert_byte(code, source);
            code.local_get(RESULT);
            insert_byte(code, destination);
        }
        Operation::MemoryExchangeAddByte { address, source } => {
            memory::load_narrow_value(code, address, SmallWidth::Byte, imports, exit_depth);
            code.local_set(LHS);
            byte_value(code, ByteValue::Register(source));
            code.local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_add()
                .i32_const(0xff)
                .i32_and()
                .local_set(RESULT)
                .local_get(LHS)
                .local_get(RHS);
            memory::store_byte_result(code, address, imports, exit_depth);
            code.local_set(RHS).local_set(LHS);
            arithmetic_flags(code, BinaryKind::Add, CarryFlag::Calculate, 7);
            code.local_get(LHS);
            insert_byte(code, source);
            store = true;
        }
        Operation::CompareExchange {
            destination,
            source,
        } => {
            code.local_get(register(Register32::Eax))
                .local_set(LHS)
                .local_get(register(destination))
                .local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_sub()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 31);
            code.local_get(RESULT)
                .i32_eqz()
                .if_(BlockType::Empty)
                .local_get(register(source))
                .local_set(register(destination))
                .else_()
                .local_get(RHS)
                .local_set(register(Register32::Eax))
                .end();
        }
        Operation::CompareExchangeWord {
            destination,
            source,
        } => {
            word_value(code, WordValue::Register(Register32::Eax));
            code.local_set(LHS);
            word_value(code, WordValue::Register(destination));
            code.local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_sub()
                .i32_const(0xffff)
                .i32_and()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 15);
            code.local_get(RESULT).i32_eqz().if_(BlockType::Empty);
            word_value(code, WordValue::Register(source));
            code.local_get(register(destination))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(destination))
                .else_()
                .local_get(RHS)
                .local_get(register(Register32::Eax))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(Register32::Eax))
                .end();
        }
        Operation::MemoryCompareExchange { address, source } => {
            memory::load_result(code, address, imports, exit_depth);
            code.local_get(register(Register32::Eax))
                .local_set(LHS)
                .local_get(RESULT)
                .local_set(RHS)
                .local_get(register(source))
                .local_get(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_eq()
                .select()
                .local_set(RESULT)
                .local_get(LHS)
                .local_get(RHS);
            memory::store_result(code, address, imports, exit_depth);
            code.local_set(RHS)
                .local_set(LHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_sub()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 31);
            code.local_get(RESULT)
                .if_(BlockType::Empty)
                .local_get(RHS)
                .local_set(register(Register32::Eax))
                .end();
            store = true;
        }
        Operation::CompareExchangeByte {
            destination,
            source,
        } => {
            byte_value(code, ByteValue::Register(ByteRegister::Al));
            code.local_set(LHS);
            byte_value(code, ByteValue::Register(destination));
            code.local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_sub()
                .i32_const(0xff)
                .i32_and()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 7);
            code.local_get(RESULT).i32_eqz().if_(BlockType::Empty);
            byte_value(code, ByteValue::Register(source));
            insert_byte(code, destination);
            code.else_().local_get(RHS);
            insert_byte(code, ByteRegister::Al);
            code.end();
        }
        Operation::MemoryCompareExchangeByte { address, source } => {
            memory::load_narrow_value(code, address, SmallWidth::Byte, imports, exit_depth);
            code.local_set(RHS);
            byte_value(code, ByteValue::Register(ByteRegister::Al));
            code.local_set(LHS);
            byte_value(code, ByteValue::Register(source));
            code.local_get(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_eq()
                .select()
                .local_set(RESULT)
                .local_get(LHS)
                .local_get(RHS);
            memory::store_byte_result(code, address, imports, exit_depth);
            code.local_set(RHS)
                .local_set(LHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_sub()
                .i32_const(0xff)
                .i32_and()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 7);
            code.local_get(RESULT).if_(BlockType::Empty).local_get(RHS);
            insert_byte(code, ByteRegister::Al);
            code.end();
            store = true;
        }
        Operation::ExchangeByte { left, right } => {
            byte_value(code, ByteValue::Register(left));
            byte_value(code, ByteValue::Register(right));
            insert_byte(code, left);
            insert_byte(code, right);
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
        Operation::UnaryWord { kind, destination } => unary_word(code, kind, destination),
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
        Operation::RotateWord {
            kind,
            destination,
            count,
        } => rotate_word(code, kind, destination, count),
        Operation::RotateThroughCarryWord {
            kind,
            destination,
            count,
        } => rotate_through_carry_word(code, kind, destination, count),
        Operation::ShiftWord {
            kind,
            destination,
            count,
        } => shift_word(code, kind, destination, count),
        Operation::ShiftByte {
            kind,
            destination,
            count,
        } => shift_byte(code, kind, destination, count),
        Operation::MemoryShiftByte { kind, address } => {
            shift_memory_byte(code, kind, address, imports, exit_depth);
            store = true;
        }
        Operation::MemoryShiftByteImmediate {
            kind,
            address,
            count,
        } => {
            shift_memory_byte_immediate(code, kind, address, count, imports, exit_depth);
            store = true;
        }
        Operation::MemoryShiftByteCl { kind, address } => {
            shift_memory_byte_cl(code, kind, address, imports, exit_depth);
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
        Operation::DoubleShift {
            kind,
            destination,
            source,
            count,
        } => double_shift(code, kind, destination, source, count),
        Operation::DoubleShiftWord {
            kind,
            destination,
            source,
            count,
        } => double_shift_word(code, kind, destination, source, count),
        Operation::MemoryDoubleShift {
            kind,
            address,
            source,
            count,
        } => {
            double_shift_memory(code, kind, address, source, count, imports, exit_depth);
            store = true;
        }
        Operation::BitTest {
            kind,
            destination,
            index,
        } => bit_test(code, kind, destination, index),
        Operation::MemoryBitMutation {
            kind,
            address,
            index,
        } => {
            memory_bit_mutation(code, kind, address, index, imports, exit_depth);
            store = true;
        }
        Operation::ReadBitTest { address, index } => {
            memory::address_value(code, address);
            if let BitIndex::Register(source) = index {
                code.local_get(register(source))
                    .i32_const(5)
                    .i32_shr_s()
                    .i32_const(2)
                    .i32_shl()
                    .i32_add();
            }
            code.local_set(ADDRESS);
            match index {
                BitIndex::Register(source) => {
                    code.local_get(register(source));
                }
                BitIndex::Immediate(raw) => {
                    code.i32_const(i32::from(raw));
                }
            }
            code.i32_const(31).i32_and();
            memory::load_result_at_address(code, imports, exit_depth);
            code.local_set(RHS)
                .local_get(FLAGS)
                .i32_const(0x442)
                .i32_and()
                .local_get(RESULT)
                .local_get(RHS)
                .i32_shr_u()
                .i32_const(1)
                .i32_and()
                .i32_or()
                .local_set(FLAGS);
        }
        Operation::ByteRotateImmediate {
            kind,
            destination,
            count,
        } => byte_rotate_immediate(code, kind, destination, count),
        Operation::ByteRotateCl { kind, destination } => byte_rotate_cl(code, kind, destination),
        Operation::ByteRotateOne { kind, destination } => byte_rotate_one(code, kind, destination),
        Operation::ByteRotateThroughCarryOne { kind, destination } => {
            byte_rotate_through_carry_one(code, kind, destination);
        }
        Operation::ByteRotateThroughCarryImmediate {
            kind,
            destination,
            count,
        } => byte_rotate_through_carry(code, kind, destination, ShiftCount::Immediate(count)),
        Operation::ByteRotateThroughCarryCl { kind, destination } => {
            byte_rotate_through_carry_cl(code, kind, destination);
        }
        Operation::MemoryByteRotateThroughCarryOne { kind, address } => {
            memory::load_narrow_value(code, address, SmallWidth::Byte, imports, exit_depth);
            code.local_set(LHS)
                .local_get(FLAGS)
                .i32_const(1)
                .i32_and()
                .local_set(RHS);
            byte_rotate_through_carry_one_value(code, kind);
            code.local_get(LHS).local_get(RHS);
            memory::store_byte_result(code, address, imports, exit_depth);
            code.local_set(RHS).local_set(LHS);
            byte_rotate_through_carry_one_flags(code, kind);
            store = true;
        }
        Operation::MemoryByteRotateThroughCarryImmediate {
            kind,
            address,
            count,
        } => {
            byte_rotate_through_carry_memory_immediate(
                code, kind, address, count, imports, exit_depth,
            );
            store = true;
        }
        Operation::MemoryByteRotateThroughCarryCl { kind, address } => {
            byte_rotate_through_carry_memory_cl(code, kind, address, imports, exit_depth);
            store = true;
        }
        Operation::MemoryByteRotateOne { kind, address } => {
            memory::load_narrow_value(code, address, SmallWidth::Byte, imports, exit_depth);
            code.local_set(RESULT);
            byte_rotate_one_value(code, kind);
            memory::store_byte_result(code, address, imports, exit_depth);
            byte_rotate_one_flags(code, kind);
            store = true;
        }
        Operation::MemoryByteRotate {
            kind,
            address,
            count,
        } => {
            byte_rotate_memory(code, kind, address, count, imports, exit_depth);
            store = true;
        }
        Operation::RotateOne { kind, destination } => rotate_one(code, kind, destination),
        Operation::Rotate {
            kind,
            destination,
            count,
        } => rotate(code, kind, destination, count),
        Operation::MemoryRotate {
            kind,
            address,
            count,
        } => {
            rotate_memory(code, kind, address, count, imports, exit_depth);
            store = true;
        }
        Operation::RotateThroughCarryOne { kind, destination } => {
            rotate_through_carry_one(code, kind, destination);
        }
        Operation::RotateThroughCarryImmediate {
            kind,
            destination,
            count,
        } => rotate_through_carry_immediate(code, kind, destination, count),
        Operation::RotateThroughCarryCl { kind, destination } => {
            rotate_through_carry_cl(code, kind, destination);
        }
        Operation::MemoryRotateThroughCarryOne { kind, address } => {
            memory::load_result(code, address, imports, exit_depth);
            code.local_get(RESULT)
                .local_set(LHS)
                .local_get(FLAGS)
                .i32_const(1)
                .i32_and()
                .local_set(RHS);
            rotate_through_carry_one_value(code, kind);
            code.local_get(LHS).local_get(RHS);
            memory::store_result(code, address, imports, exit_depth);
            code.local_set(RHS).local_set(LHS);
            rotate_through_carry_one_flags(code, kind);
            store = true;
        }
        Operation::MemoryRotateThroughCarryImmediate {
            kind,
            address,
            count,
        } => {
            memory_rotate_through_carry_immediate(code, kind, address, count, imports, exit_depth);
            store = true;
        }
        Operation::MemoryRotateThroughCarryCl { kind, address } => {
            memory_rotate_through_carry_cl(code, kind, address, imports, exit_depth);
            store = true;
        }
        Operation::MemoryRotateOne { kind, address } => {
            memory::load_result(code, address, imports, exit_depth);
            rotate_one_value(code, kind);
            memory::store_result(code, address, imports, exit_depth);
            rotate_one_flags(code, kind);
            store = true;
        }
        Operation::SignedMultiply {
            destination,
            source,
            immediate,
        } => {
            signed_multiply(code, destination, source, immediate, imports, exit_depth);
        }
        Operation::MultiplyAccumulator { kind, source } => {
            accumulator_multiply(code, kind, register(source));
        }
        Operation::ByteMultiplyAccumulator { kind, source } => {
            byte_accumulator_multiply(code, kind, source);
        }
        Operation::ReadByteMultiplyAccumulator { kind, address } => {
            memory::load_narrow_value(code, address, SmallWidth::Byte, imports, exit_depth);
            code.local_set(RHS);
            byte_value(code, ByteValue::Register(ByteRegister::Al));
            code.local_set(LHS);
            byte_accumulator_multiply_value(code, kind);
        }
        Operation::ReadMultiplyAccumulator { kind, address } => {
            memory::load_result(code, address, imports, exit_depth);
            accumulator_multiply(code, kind, RESULT);
        }
        Operation::DivideAccumulator { kind, source } => {
            accumulator_divide(code, kind, register(source), exit_depth);
        }
        Operation::ByteDivideAccumulator { kind, source } => {
            byte_accumulator_divide(code, kind, source, exit_depth);
        }
        Operation::ReadByteDivideAccumulator { kind, address } => {
            memory::load_narrow_value(code, address, SmallWidth::Byte, imports, exit_depth);
            byte_accumulator_divide_value(code, kind, exit_depth);
        }
        Operation::ReadDivideAccumulator { kind, address } => {
            memory::load_result(code, address, imports, exit_depth);
            accumulator_divide(code, kind, RESULT, exit_depth);
        }
        Operation::BitScan {
            kind,
            destination,
            source,
        } => {
            bit_scan(code, kind, destination, register(source));
        }
        Operation::ReadBitScan {
            kind,
            destination,
            address,
        } => {
            memory::load_result(code, address, imports, exit_depth);
            bit_scan(code, kind, destination, RESULT);
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
        Operation::CountBranch { kind, target } => {
            let ecx = register(Register32::Ecx);
            if kind == CountBranchKind::EcxZero {
                code.local_get(ecx).i32_eqz();
            } else {
                code.local_get(ecx)
                    .i32_const(1)
                    .i32_sub()
                    .local_tee(ecx)
                    .i32_const(0)
                    .i32_ne();
                match kind {
                    CountBranchKind::LoopEqual | CountBranchKind::LoopNotEqual => {
                        control::condition(
                            code,
                            if kind == CountBranchKind::LoopEqual {
                                Condition::Equal
                            } else {
                                Condition::NotEqual
                            },
                        );
                        code.i32_and();
                    }
                    _ => {}
                }
            }
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
        Operation::PushFlags => {
            code.local_get(FLAGS)
                .i32_const(0x00fc_ffff)
                .i32_and()
                .local_set(RESULT);
            memory::store_result(
                code,
                EffectiveAddress {
                    base: Some(Register32::Esp),
                    index: None,
                    scale: 1,
                    displacement: (-4_i32) as u32,
                },
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
        Operation::Leave => {
            memory::load_result(
                code,
                EffectiveAddress {
                    base: Some(Register32::Ebp),
                    index: None,
                    scale: 1,
                    displacement: 0,
                },
                imports,
                exit_depth,
            );
            code.local_get(ADDRESS)
                .i32_const(4)
                .i32_add()
                .local_set(register(Register32::Esp))
                .local_get(RESULT)
                .local_set(register(Register32::Ebp));
        }
        Operation::Return { stack_adjust } => {
            memory::pop_return(code, stack_adjust, imports, exit_depth);
        }
    }
    if !matches!(
        instruction.operation(),
        Operation::Jump { .. }
            | Operation::ConditionalJump { .. }
            | Operation::CountBranch { .. }
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

fn word_value(code: &mut InstructionSink<'_>, value: WordValue) {
    match value {
        WordValue::Register(source) => {
            code.local_get(register(source)).i32_const(0xffff).i32_and();
        }
        WordValue::Immediate(value) => {
            code.i32_const(i32::from(value));
        }
    }
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

fn string_pointer(code: &mut InstructionSink<'_>, pointer: Register32) {
    code.local_get(register(pointer))
        .i32_const(-1)
        .i32_const(1)
        .local_get(FLAGS)
        .i32_const(0x400)
        .i32_and()
        .select()
        .i32_add()
        .local_set(register(pointer));
}

fn word_string_pointer(code: &mut InstructionSink<'_>, pointer: Register32) {
    code.local_get(register(pointer))
        .i32_const(-2)
        .i32_const(2)
        .local_get(FLAGS)
        .i32_const(0x400)
        .i32_and()
        .select()
        .i32_add()
        .local_set(register(pointer));
}

fn dword_string_pointer(code: &mut InstructionSink<'_>, pointer: Register32) {
    code.local_get(register(pointer))
        .i32_const(-4)
        .i32_const(4)
        .local_get(FLAGS)
        .i32_const(0x400)
        .i32_and()
        .select()
        .i32_add()
        .local_set(register(pointer));
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

fn unary_word(code: &mut InstructionSink<'_>, kind: UnaryKind, destination: Register32) {
    word_value(code, WordValue::Register(destination));
    code.local_set(LHS);
    let (binary, carry) = match kind {
        UnaryKind::Not => {
            code.local_get(LHS)
                .i32_const(0xffff)
                .i32_xor()
                .local_get(register(destination))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
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
    code.i32_const(0xffff).i32_and().local_set(RESULT);
    arithmetic_flags(code, binary, carry, 15);
    code.local_get(RESULT)
        .local_get(register(destination))
        .i32_const(!0xffff)
        .i32_and()
        .i32_or()
        .local_set(register(destination));
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

fn rotate_word(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
    destination: Register32,
    count: ShiftCount,
) {
    word_value(code, WordValue::Register(destination));
    code.local_set(RESULT);
    shift_count(code, count);
    code.local_tee(RHS)
        .if_(BlockType::Empty)
        .local_get(RHS)
        .i32_const(15)
        .i32_and()
        .local_set(LHS)
        .local_get(RESULT);
    match kind {
        RotateKind::Left => {
            code.local_get(LHS).i32_shl();
        }
        RotateKind::Right => {
            code.i32_const(16).local_get(LHS).i32_sub().i32_shl();
        }
    }
    code.local_get(RESULT);
    match kind {
        RotateKind::Left => {
            code.i32_const(16).local_get(LHS).i32_sub().i32_shr_u();
        }
        RotateKind::Right => {
            code.local_get(LHS).i32_shr_u();
        }
    }
    code.i32_or()
        .i32_const(0xffff)
        .i32_and()
        .local_set(RESULT)
        .local_get(FLAGS)
        .i32_const(!0x801)
        .i32_and()
        .local_get(RESULT)
        .i32_const(if kind == RotateKind::Left { 0 } else { 15 })
        .i32_shr_u()
        .i32_const(1)
        .i32_and()
        .i32_or()
        .local_set(FLAGS);
    // of uses the masked count, not the rotation distance; undefined multi-count of is cleared.
    code.local_get(RHS)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Empty)
        .local_get(FLAGS)
        .local_get(RESULT)
        .i32_const(15)
        .i32_shr_u()
        .local_get(RESULT)
        .i32_const(if kind == RotateKind::Left { 0 } else { 14 })
        .i32_shr_u()
        .i32_xor()
        .i32_const(1)
        .i32_and()
        .i32_const(11)
        .i32_shl()
        .i32_or()
        .local_set(FLAGS)
        .end()
        .local_get(RESULT)
        .local_get(register(destination))
        .i32_const(!0xffff)
        .i32_and()
        .i32_or()
        .local_set(register(destination))
        .end();
}

fn rotate_through_carry_word(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
    destination: Register32,
    count: ShiftCount,
) {
    word_value(code, WordValue::Register(destination));
    code.local_set(RESULT);
    // keep the original masked count separate from the 17-bit rotation distance.
    shift_count(code, count);
    code.local_tee(RHS)
        .i32_const(17)
        .i32_rem_u()
        .local_tee(LHS)
        .if_(BlockType::Empty)
        .local_get(RESULT)
        .local_get(FLAGS)
        .i32_const(1)
        .i32_and()
        .i32_const(16)
        .i32_shl()
        .i32_or()
        .local_set(RESULT)
        .local_get(RESULT);
    match kind {
        RotateKind::Left => {
            code.local_get(LHS).i32_shl();
        }
        RotateKind::Right => {
            code.i32_const(17).local_get(LHS).i32_sub().i32_shl();
        }
    }
    code.local_get(RESULT);
    match kind {
        RotateKind::Left => {
            code.i32_const(17).local_get(LHS).i32_sub().i32_shr_u();
        }
        RotateKind::Right => {
            code.local_get(LHS).i32_shr_u();
        }
    }
    code.i32_or()
        .i32_const(0x1ffff)
        .i32_and()
        .local_set(RESULT)
        .local_get(FLAGS)
        .i32_const(!0x801)
        .i32_and()
        .local_get(RESULT)
        .i32_const(16)
        .i32_shr_u()
        .i32_or()
        .local_set(FLAGS)
        .local_get(RHS)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Empty)
        .local_get(FLAGS)
        .local_get(RESULT)
        .i32_const(15)
        .i32_shr_u()
        .local_get(RESULT)
        .i32_const(if kind == RotateKind::Left { 16 } else { 14 })
        .i32_shr_u()
        .i32_xor()
        .i32_const(1)
        .i32_and()
        .i32_const(11)
        .i32_shl()
        .i32_or()
        .local_set(FLAGS)
        .end()
        .local_get(RESULT)
        .i32_const(0xffff)
        .i32_and()
        .local_get(register(destination))
        .i32_const(!0xffff)
        .i32_and()
        .i32_or()
        .local_set(register(destination))
        .end();
}

fn shift_word(
    code: &mut InstructionSink<'_>,
    kind: ShiftKind,
    destination: Register32,
    count: ShiftCount,
) {
    word_value(code, WordValue::Register(destination));
    code.local_set(LHS);
    shift_count(code, count);
    code.local_tee(RHS).if_(BlockType::Empty);
    shift_word_value(code, kind);
    shift_word_flags(code, kind);
    code.local_get(RESULT)
        .local_get(register(destination))
        .i32_const(!0xffff)
        .i32_and()
        .i32_or()
        .local_set(register(destination))
        .end();
}

fn shift_word_value(code: &mut InstructionSink<'_>, kind: ShiftKind) {
    code.local_get(LHS);
    if kind == ShiftKind::Sar {
        code.i32_const(16).i32_shl().i32_const(16).i32_shr_s();
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
    code.i32_const(0xffff).i32_and().local_set(RESULT);
}

fn shift_word_flags(code: &mut InstructionSink<'_>, kind: ShiftKind) {
    // undefined af, multi-bit of and shl/shr carry at counts >= 16 are cleared by this profile.
    logical_flags(code, 15);
    code.local_get(RHS)
        .i32_const(16)
        .i32_lt_u()
        .if_(BlockType::Empty)
        .local_get(FLAGS)
        .local_get(LHS);
    if kind == ShiftKind::Shl {
        code.i32_const(16).local_get(RHS).i32_sub();
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
            .i32_const(15)
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
                .i32_const(15)
                .i32_shr_u()
                .local_get(FLAGS)
                .i32_const(1)
                .i32_and()
                .i32_xor();
        } else {
            code.local_get(LHS).i32_const(15).i32_shr_u();
        }
        code.i32_const(11).i32_shl().i32_or().local_set(FLAGS).end();
    }
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

fn shift_memory_byte_immediate(
    code: &mut InstructionSink<'_>,
    kind: ShiftKind,
    address: EffectiveAddress,
    count: u8,
    imports: memory::Imports,
    exit_depth: u32,
) {
    let count = i32::from(count & 31);
    memory::load_narrow_value(code, address, SmallWidth::Byte, imports, exit_depth);
    code.local_set(LHS);
    if count == 0 {
        code.local_get(LHS).local_set(RESULT);
    } else {
        code.i32_const(count).local_set(RHS);
        shift_byte_value(code, kind);
        // checked store clobbers operand scratch; preserve the lossy original byte.
        code.local_get(LHS);
    }
    memory::store_byte_result(code, address, imports, exit_depth);
    if count != 0 {
        code.local_set(LHS).i32_const(count).local_set(RHS);
        shift_byte_flags(code, kind);
    }
}

fn shift_memory_byte_cl(
    code: &mut InstructionSink<'_>,
    kind: ShiftKind,
    address: EffectiveAddress,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::load_narrow_value(code, address, SmallWidth::Byte, imports, exit_depth);
    code.local_set(LHS);
    shift_count(code, ShiftCount::Cl);
    code.local_set(RHS);
    shift_byte_value(code, kind);
    // checked store clobbers both the original byte and runtime count scratch.
    code.local_get(LHS).local_get(RHS);
    memory::store_byte_result(code, address, imports, exit_depth);
    code.local_set(RHS).local_set(LHS);
    code.local_get(RHS).if_(BlockType::Empty);
    shift_byte_flags(code, kind);
    code.end();
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

fn double_shift_word(
    code: &mut InstructionSink<'_>,
    kind: DoubleShiftKind,
    destination: Register32,
    source: Register32,
    count: ShiftCount,
) {
    word_value(code, WordValue::Register(destination));
    code.local_set(LHS);
    shift_count(code, count);
    code.local_set(RHS);
    word_value(code, WordValue::Register(source));
    code.local_set(RESULT)
        .local_get(RHS)
        .if_(BlockType::Empty)
        .local_get(RHS)
        .i32_const(16)
        .i32_gt_u()
        .if_(BlockType::Empty);
    // above the word width, x86 leaves result and all arithmetic flags undefined.
    // this profile chooses zero for both; do not derive ZF/PF from that zero result.
    code.i32_const(0)
        .local_set(RESULT)
        .local_get(FLAGS)
        .i32_const(0x400)
        .i32_and()
        .i32_const(2)
        .i32_or()
        .local_set(FLAGS)
        .else_();
    double_shift_word_value(code, kind);
    double_shift_word_flags(code, kind);
    code.end()
        .local_get(RESULT)
        .local_get(register(destination))
        .i32_const(!0xffff)
        .i32_and()
        .i32_or()
        .local_set(register(destination))
        .end();
}

fn double_shift_word_value(code: &mut InstructionSink<'_>, kind: DoubleShiftKind) {
    code.local_get(LHS).local_get(RHS);
    match kind {
        DoubleShiftKind::Left => {
            code.i32_shl()
                .local_get(RESULT)
                .i32_const(16)
                .local_get(RHS)
                .i32_sub()
                .i32_shr_u();
        }
        DoubleShiftKind::Right => {
            code.i32_shr_u()
                .local_get(RESULT)
                .i32_const(16)
                .local_get(RHS)
                .i32_sub()
                .i32_shl();
        }
    }
    code.i32_or().i32_const(0xffff).i32_and().local_set(RESULT);
}

fn double_shift_word_flags(code: &mut InstructionSink<'_>, kind: DoubleShiftKind) {
    // only counts 1..16 reach this branch; AF and multi-bit OF clear by policy.
    logical_flags(code, 15);
    code.local_get(FLAGS).local_get(LHS);
    match kind {
        DoubleShiftKind::Left => {
            code.i32_const(16).local_get(RHS).i32_sub();
        }
        DoubleShiftKind::Right => {
            code.local_get(RHS).i32_const(1).i32_sub();
        }
    }
    code.i32_shr_u()
        .i32_const(1)
        .i32_and()
        .i32_or()
        .local_set(FLAGS)
        .local_get(RHS)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Empty)
        .local_get(FLAGS)
        .local_get(LHS)
        .local_get(RESULT)
        .i32_xor()
        .i32_const(15)
        .i32_shr_u()
        .i32_const(11)
        .i32_shl()
        .i32_or()
        .local_set(FLAGS)
        .end();
}

fn double_shift(
    code: &mut InstructionSink<'_>,
    kind: DoubleShiftKind,
    destination: Register32,
    source: Register32,
    count: ShiftCount,
) {
    code.local_get(register(destination)).local_set(LHS);
    shift_count(code, count);
    code.local_set(RHS)
        .local_get(register(source))
        .local_set(RESULT)
        .local_get(RHS)
        .if_(BlockType::Empty);
    double_shift_value(code, kind);
    double_shift_flags(code, kind);
    code.local_get(RESULT)
        .local_set(register(destination))
        .end();
}

fn double_shift_value(code: &mut InstructionSink<'_>, kind: DoubleShiftKind) {
    code.local_get(LHS).local_get(RHS);
    match kind {
        DoubleShiftKind::Left => {
            code.i32_shl()
                .local_get(RESULT)
                .i32_const(32)
                .local_get(RHS)
                .i32_sub()
                .i32_shr_u();
        }
        DoubleShiftKind::Right => {
            code.i32_shr_u()
                .local_get(RESULT)
                .i32_const(32)
                .local_get(RHS)
                .i32_sub()
                .i32_shl();
        }
    }
    code.i32_or().local_set(RESULT);
}

fn double_shift_flags(code: &mut InstructionSink<'_>, kind: DoubleShiftKind) {
    // af and multi-bit of are undefined; this profile clears them.
    logical_flags(code, 31);
    code.local_get(FLAGS).local_get(LHS);
    match kind {
        DoubleShiftKind::Left => {
            code.i32_const(32).local_get(RHS).i32_sub();
        }
        DoubleShiftKind::Right => {
            code.local_get(RHS).i32_const(1).i32_sub();
        }
    }
    code.i32_shr_u()
        .i32_const(1)
        .i32_and()
        .i32_or()
        .local_set(FLAGS)
        .local_get(RHS)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Empty)
        .local_get(FLAGS)
        .local_get(LHS)
        .local_get(RESULT)
        .i32_xor()
        .i32_const(31)
        .i32_shr_u()
        .i32_const(11)
        .i32_shl()
        .i32_or()
        .local_set(FLAGS)
        .end();
}

fn double_shift_memory(
    code: &mut InstructionSink<'_>,
    kind: DoubleShiftKind,
    address: EffectiveAddress,
    source: Register32,
    count: ShiftCount,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::load_result(code, address, imports, exit_depth);
    code.local_get(RESULT).local_set(LHS);
    shift_count(code, count);
    code.local_tee(RHS)
        .if_(BlockType::Empty)
        .local_get(register(source))
        .local_set(RESULT);
    double_shift_value(code, kind);
    code.end();
    // store validation clobbers scratch; preserve the old word and count until flags commit.
    code.local_get(LHS).local_get(RHS);
    memory::store_result(code, address, imports, exit_depth);
    code.local_set(RHS)
        .local_set(LHS)
        .local_get(RHS)
        .if_(BlockType::Empty);
    double_shift_flags(code, kind);
    code.end();
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

fn byte_rotate_immediate(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
    destination: ByteRegister,
    count: u8,
) {
    let count = count & 31;
    if count == 0 {
        return;
    }
    if count == 1 {
        byte_rotate_one(code, kind, destination);
        return;
    }
    byte_value(code, ByteValue::Register(destination));
    code.local_set(RESULT);
    let distance = count & 7;
    if distance != 0 {
        let (left_shift, right_shift) = match kind {
            RotateKind::Left => (distance, 8 - distance),
            RotateKind::Right => (8 - distance, distance),
        };
        code.local_get(RESULT)
            .i32_const(i32::from(left_shift))
            .i32_shl()
            .local_get(RESULT)
            .i32_const(i32::from(right_shift))
            .i32_shr_u()
            .i32_or()
            .i32_const(0xff)
            .i32_and()
            .local_set(RESULT);
    }
    code.local_get(FLAGS)
        .i32_const(!0x801)
        .i32_and()
        .local_get(RESULT)
        .i32_const(if kind == RotateKind::Left { 0 } else { 7 })
        .i32_shr_u()
        .i32_const(1)
        .i32_and()
        .i32_or()
        .local_set(FLAGS)
        .local_get(RESULT);
    insert_byte(code, destination);
}

fn byte_rotate_cl(code: &mut InstructionSink<'_>, kind: RotateKind, destination: ByteRegister) {
    byte_value(code, ByteValue::Register(destination));
    code.local_set(RESULT);
    shift_count(code, ShiftCount::Cl);
    code.local_tee(RHS)
        .if_(BlockType::Empty)
        .local_get(RHS)
        .i32_const(7)
        .i32_and()
        .local_set(LHS)
        .local_get(RESULT);
    match kind {
        RotateKind::Left => {
            code.local_get(LHS).i32_shl();
        }
        RotateKind::Right => {
            code.i32_const(8).local_get(LHS).i32_sub().i32_shl();
        }
    }
    code.local_get(RESULT);
    match kind {
        RotateKind::Left => {
            code.i32_const(8).local_get(LHS).i32_sub().i32_shr_u();
        }
        RotateKind::Right => {
            code.local_get(LHS).i32_shr_u();
        }
    }
    code.i32_or()
        .i32_const(0xff)
        .i32_and()
        .local_set(RESULT)
        .local_get(RHS)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Empty);
    byte_rotate_one_flags(code, kind);
    code.else_()
        .local_get(FLAGS)
        .i32_const(!0x801)
        .i32_and()
        .local_get(RESULT)
        .i32_const(if kind == RotateKind::Left { 0 } else { 7 })
        .i32_shr_u()
        .i32_const(1)
        .i32_and()
        .i32_or()
        .local_set(FLAGS)
        .end()
        .local_get(RESULT);
    insert_byte(code, destination);
    code.end();
}

fn byte_rotate_one(code: &mut InstructionSink<'_>, kind: RotateKind, destination: ByteRegister) {
    byte_value(code, ByteValue::Register(destination));
    code.local_set(RESULT);
    byte_rotate_one_value(code, kind);
    code.local_get(RESULT);
    insert_byte(code, destination);
    byte_rotate_one_flags(code, kind);
}

fn byte_rotate_one_value(code: &mut InstructionSink<'_>, kind: RotateKind) {
    let (left_shift, right_shift) = match kind {
        RotateKind::Left => (1, 7),
        RotateKind::Right => (7, 1),
    };
    code.local_get(RESULT)
        .i32_const(left_shift)
        .i32_shl()
        .local_get(RESULT)
        .i32_const(right_shift)
        .i32_shr_u()
        .i32_or()
        .i32_const(0xff)
        .i32_and()
        .local_set(RESULT);
}

fn byte_rotate_one_flags(code: &mut InstructionSink<'_>, kind: RotateKind) {
    code.local_get(FLAGS)
        .i32_const(!0x801)
        .i32_and()
        .local_get(RESULT)
        .i32_const(if kind == RotateKind::Left { 0 } else { 7 })
        .i32_shr_u()
        .i32_const(1)
        .i32_and()
        .i32_or()
        .local_get(RESULT)
        .i32_const(7)
        .i32_shr_u()
        .local_get(RESULT)
        .i32_const(if kind == RotateKind::Left { 0 } else { 6 })
        .i32_shr_u()
        .i32_const(1)
        .i32_and()
        .i32_xor()
        .i32_const(11)
        .i32_shl()
        .i32_or()
        .local_set(FLAGS);
}

fn byte_rotate_memory(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
    address: EffectiveAddress,
    count: ShiftCount,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::load_narrow_value(code, address, SmallWidth::Byte, imports, exit_depth);
    code.local_set(RESULT);
    shift_count(code, count);
    code.local_tee(RHS)
        .i32_const(7)
        .i32_and()
        .local_set(LHS)
        .local_get(RESULT);
    match kind {
        RotateKind::Left => {
            code.local_get(LHS).i32_shl();
        }
        RotateKind::Right => {
            code.i32_const(8).local_get(LHS).i32_sub().i32_shl();
        }
    }
    code.local_get(RESULT);
    match kind {
        RotateKind::Left => {
            code.i32_const(8).local_get(LHS).i32_sub().i32_shr_u();
        }
        RotateKind::Right => {
            code.local_get(LHS).i32_shr_u();
        }
    }
    code.i32_or().i32_const(0xff).i32_and().local_set(RESULT);
    // flags use the masked count, preserved across store validation.
    code.local_get(RHS);
    memory::store_byte_result(code, address, imports, exit_depth);
    code.local_set(RHS).local_get(RHS).if_(BlockType::Empty);
    code.local_get(RHS)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Empty);
    byte_rotate_one_flags(code, kind);
    code.else_()
        .local_get(FLAGS)
        .i32_const(!0x801)
        .i32_and()
        .local_get(RESULT)
        .i32_const(if kind == RotateKind::Left { 0 } else { 7 })
        .i32_shr_u()
        .i32_const(1)
        .i32_and()
        .i32_or()
        .local_set(FLAGS)
        .end()
        .end();
}

fn rotate(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
    destination: Register32,
    count: ShiftCount,
) {
    code.local_get(register(destination)).local_set(RESULT);
    shift_count(code, count);
    code.local_tee(RHS).if_(BlockType::Empty);
    rotate_value(code, kind);
    code.local_tee(RESULT).local_set(register(destination));
    rotate_flags(code, kind);
    code.end();
}

fn rotate_memory(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
    address: EffectiveAddress,
    count: ShiftCount,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::load_result(code, address, imports, exit_depth);
    shift_count(code, count);
    code.local_tee(RHS).if_(BlockType::Empty);
    rotate_value(code, kind);
    code.local_set(RESULT).end();
    // store validation clobbers the count; flags publish only after a successful store.
    code.local_get(RHS);
    memory::store_result(code, address, imports, exit_depth);
    code.local_set(RHS).local_get(RHS).if_(BlockType::Empty);
    rotate_flags(code, kind);
    code.end();
}

fn rotate_value(code: &mut InstructionSink<'_>, kind: RotateKind) {
    code.local_get(RESULT).local_get(RHS);
    match kind {
        RotateKind::Left => {
            code.i32_rotl();
        }
        RotateKind::Right => {
            code.i32_rotr();
        }
    }
}

fn rotate_flags(code: &mut InstructionSink<'_>, kind: RotateKind) {
    code.local_get(RHS)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Empty);
    rotate_one_flags(code, kind);
    code.else_()
        .local_get(FLAGS)
        .i32_const(!0x801)
        .i32_and()
        .local_get(RESULT);
    match kind {
        RotateKind::Left => {
            code.i32_const(1).i32_and();
        }
        RotateKind::Right => {
            code.i32_const(31).i32_shr_u();
        }
    }
    code.i32_or().local_set(FLAGS).end();
}

fn rotate_one(code: &mut InstructionSink<'_>, kind: RotateKind, destination: Register32) {
    code.local_get(register(destination)).local_set(RESULT);
    rotate_one_value(code, kind);
    code.local_get(RESULT).local_set(register(destination));
    rotate_one_flags(code, kind);
}

fn rotate_one_value(code: &mut InstructionSink<'_>, kind: RotateKind) {
    code.local_get(RESULT).i32_const(1);
    match kind {
        RotateKind::Left => {
            code.i32_rotl();
        }
        RotateKind::Right => {
            code.i32_rotr();
        }
    }
    code.local_set(RESULT);
}

fn rotate_one_flags(code: &mut InstructionSink<'_>, kind: RotateKind) {
    code.local_get(FLAGS)
        .i32_const(!0x801)
        .i32_and()
        .local_get(RESULT);
    match kind {
        RotateKind::Left => {
            code.i32_const(1).i32_and();
        }
        RotateKind::Right => {
            code.i32_const(31).i32_shr_u();
        }
    }
    code.i32_or()
        .local_get(RESULT)
        .i32_const(31)
        .i32_shr_u()
        .local_get(RESULT);
    match kind {
        RotateKind::Left => {
            code.i32_const(1).i32_and();
        }
        RotateKind::Right => {
            code.i32_const(30).i32_shr_u().i32_const(1).i32_and();
        }
    }
    code.i32_xor()
        .i32_const(11)
        .i32_shl()
        .i32_or()
        .local_set(FLAGS);
}

fn byte_rotate_through_carry_one(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
    destination: ByteRegister,
) {
    byte_value(code, ByteValue::Register(destination));
    code.local_set(LHS)
        .local_get(FLAGS)
        .i32_const(1)
        .i32_and()
        .local_set(RHS);
    byte_rotate_through_carry_one_value(code, kind);
    code.local_get(RESULT);
    insert_byte(code, destination);
    byte_rotate_through_carry_one_flags(code, kind);
}

fn byte_rotate_through_carry_one_value(code: &mut InstructionSink<'_>, kind: RotateKind) {
    code.local_get(LHS).i32_const(1);
    match kind {
        RotateKind::Left => {
            code.i32_shl();
        }
        RotateKind::Right => {
            code.i32_shr_u();
        }
    }
    code.local_get(RHS);
    if kind == RotateKind::Right {
        code.i32_const(7).i32_shl();
    }
    code.i32_or().i32_const(0xff).i32_and().local_set(RESULT);
}

fn byte_rotate_through_carry_one_flags(code: &mut InstructionSink<'_>, kind: RotateKind) {
    code.local_get(FLAGS)
        .i32_const(!0x801)
        .i32_and()
        .local_get(LHS);
    match kind {
        RotateKind::Left => {
            code.i32_const(7).i32_shr_u();
        }
        RotateKind::Right => {
            code.i32_const(1).i32_and();
        }
    }
    code.i32_or();
    match kind {
        RotateKind::Left => {
            code.local_get(RESULT)
                .i32_const(7)
                .i32_shr_u()
                .local_get(LHS)
                .i32_const(7)
                .i32_shr_u();
        }
        RotateKind::Right => {
            code.local_get(LHS).i32_const(7).i32_shr_u().local_get(RHS);
        }
    }
    code.i32_xor()
        .i32_const(11)
        .i32_shl()
        .i32_or()
        .local_set(FLAGS);
}

fn byte_rotate_through_carry_cl(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
    destination: ByteRegister,
) {
    byte_rotate_through_carry(code, kind, destination, ShiftCount::Cl);
}

fn byte_rotate_through_carry_memory_immediate(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
    address: EffectiveAddress,
    count: u8,
    imports: memory::Imports,
    exit_depth: u32,
) {
    // masked-one instructions use the existing One path with its defined OF.
    let distance = i32::from((count & 31) % 9);
    memory::load_narrow_value(code, address, SmallWidth::Byte, imports, exit_depth);
    code.local_set(RESULT);
    if distance == 0 {
        // a zero rotation still performs the checked same-value destination write.
        memory::store_byte_result(code, address, imports, exit_depth);
        return;
    }
    code.local_get(RESULT)
        .local_get(FLAGS)
        .i32_const(1)
        .i32_and()
        .i32_const(8)
        .i32_shl()
        .i32_or()
        .local_set(RESULT)
        .local_get(RESULT)
        .i32_const(if kind == RotateKind::Left {
            distance
        } else {
            9 - distance
        })
        .i32_shl()
        .local_get(RESULT)
        .i32_const(if kind == RotateKind::Left {
            9 - distance
        } else {
            distance
        })
        .i32_shr_u()
        .i32_or()
        .i32_const(0x1ff)
        .i32_and()
        .local_tee(RESULT)
        // keep the ring's carry bit across helpers which use LHS/RHS as scratch.
        .local_get(RESULT)
        .i32_const(0xff)
        .i32_and()
        .local_set(RESULT);
    memory::store_byte_result(code, address, imports, exit_depth);
    code.local_set(RESULT)
        .local_get(FLAGS)
        .i32_const(!0x801)
        .i32_and()
        .local_get(RESULT)
        .i32_const(8)
        .i32_shr_u()
        .i32_or()
        .local_set(FLAGS);
}

fn byte_rotate_through_carry_memory_cl(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
    address: EffectiveAddress,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::load_narrow_value(code, address, SmallWidth::Byte, imports, exit_depth);
    code.local_set(RESULT)
        .local_get(register(Register32::Ecx))
        .i32_const(31)
        .i32_and()
        .local_tee(LHS)
        .i32_const(9)
        .i32_rem_u()
        .local_set(RHS)
        .local_get(RESULT)
        .local_get(FLAGS)
        .i32_const(1)
        .i32_and()
        .i32_const(8)
        .i32_shl()
        .i32_or()
        .local_set(RESULT)
        .local_get(RESULT);
    match kind {
        RotateKind::Left => {
            code.local_get(RHS).i32_shl();
        }
        RotateKind::Right => {
            code.i32_const(9).local_get(RHS).i32_sub().i32_shl();
        }
    }
    code.local_get(RESULT);
    match kind {
        RotateKind::Left => {
            code.i32_const(9).local_get(RHS).i32_sub().i32_shr_u();
        }
        RotateKind::Right => {
            code.local_get(RHS).i32_shr_u();
        }
    }
    code.i32_or()
        .i32_const(0x1ff)
        .i32_and()
        .local_set(RESULT)
        .local_get(RHS)
        .if_(BlockType::Result(ValType::I32))
        .local_get(FLAGS)
        .i32_const(!0x801)
        .i32_and()
        .local_get(RESULT)
        .i32_const(8)
        .i32_shr_u()
        .i32_or()
        .local_get(LHS)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Result(ValType::I32))
        .local_get(RESULT)
        .i32_const(7)
        .i32_shr_u()
        .local_get(RESULT)
        .i32_const(if kind == RotateKind::Left { 8 } else { 6 })
        .i32_shr_u()
        .i32_xor()
        .i32_const(1)
        .i32_and()
        .i32_const(11)
        .i32_shl()
        .else_()
        .i32_const(0)
        .end()
        .i32_or()
        .else_()
        .local_get(FLAGS)
        .end()
        // keep candidate FLAGS across helpers which use LHS/RHS as scratch.
        .local_get(RESULT)
        .i32_const(0xff)
        .i32_and()
        .local_set(RESULT);
    memory::store_byte_result(code, address, imports, exit_depth);
    code.local_set(FLAGS);
}

fn byte_rotate_through_carry(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
    destination: ByteRegister,
    count: ShiftCount,
) {
    shift_count(code, count);
    code.local_tee(RHS)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Empty);
    byte_rotate_through_carry_one(code, kind, destination);
    code.else_()
        .local_get(RHS)
        .i32_const(9)
        .i32_rem_u()
        .local_tee(LHS)
        .if_(BlockType::Empty);
    byte_value(code, ByteValue::Register(destination));
    code.local_get(FLAGS)
        .i32_const(1)
        .i32_and()
        .i32_const(8)
        .i32_shl()
        .i32_or()
        .local_set(RESULT)
        .local_get(RESULT);
    match kind {
        RotateKind::Left => {
            code.local_get(LHS).i32_shl();
        }
        RotateKind::Right => {
            code.i32_const(9).local_get(LHS).i32_sub().i32_shl();
        }
    }
    code.local_get(RESULT);
    match kind {
        RotateKind::Left => {
            code.i32_const(9).local_get(LHS).i32_sub().i32_shr_u();
        }
        RotateKind::Right => {
            code.local_get(LHS).i32_shr_u();
        }
    }
    code.i32_or()
        .i32_const(0x1ff)
        .i32_and()
        .local_set(RESULT)
        .local_get(FLAGS)
        .i32_const(!0x801)
        .i32_and()
        .local_get(RESULT)
        .i32_const(8)
        .i32_shr_u()
        .i32_or()
        .local_set(FLAGS)
        .local_get(RESULT)
        .i32_const(0xff)
        .i32_and();
    insert_byte(code, destination);
    code.end().end();
}

fn rotate_through_carry_immediate(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
    destination: Register32,
    raw: u8,
) {
    let count = i32::from(raw & 31);
    match count {
        0 => return,
        1 => {
            rotate_through_carry_one(code, kind, destination);
            return;
        }
        _ => {}
    }
    code.local_get(register(destination))
        .local_set(LHS)
        .local_get(FLAGS)
        .i32_const(1)
        .i32_and()
        .local_set(RHS)
        .local_get(LHS)
        .i32_const(count);
    match kind {
        RotateKind::Left => {
            code.i32_shl();
        }
        RotateKind::Right => {
            code.i32_shr_u();
        }
    }
    code.local_get(LHS).i32_const(33 - count);
    match kind {
        RotateKind::Left => {
            code.i32_shr_u();
        }
        RotateKind::Right => {
            code.i32_shl();
        }
    }
    let (carry_in_shift, carry_out_shift) = match kind {
        RotateKind::Left => (count - 1, 32 - count),
        RotateKind::Right => (32 - count, count - 1),
    };
    code.i32_or()
        .local_get(RHS)
        .i32_const(carry_in_shift)
        .i32_shl()
        .i32_or()
        .local_set(RESULT)
        .local_get(FLAGS)
        .i32_const(!0x801)
        .i32_and()
        .local_get(LHS)
        .i32_const(carry_out_shift)
        .i32_shr_u()
        .i32_const(1)
        .i32_and()
        .i32_or()
        .local_set(FLAGS)
        .local_get(RESULT)
        .local_set(register(destination));
}

fn memory_rotate_through_carry_immediate(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
    address: EffectiveAddress,
    raw: u8,
    imports: memory::Imports,
    exit_depth: u32,
) {
    let count = i32::from(raw & 31);
    memory::load_result(code, address, imports, exit_depth);
    if count == 0 {
        memory::store_result(code, address, imports, exit_depth);
        return;
    }
    code.local_get(RESULT)
        .local_set(LHS)
        .local_get(FLAGS)
        .i32_const(1)
        .i32_and()
        .local_set(RHS);
    if count == 1 {
        rotate_through_carry_one_value(code, kind);
        code.local_get(LHS).local_get(RHS);
        memory::store_result(code, address, imports, exit_depth);
        code.local_set(RHS).local_set(LHS);
        rotate_through_carry_one_flags(code, kind);
        return;
    }
    code.local_get(LHS).i32_const(count);
    match kind {
        RotateKind::Left => {
            code.i32_shl();
        }
        RotateKind::Right => {
            code.i32_shr_u();
        }
    }
    code.local_get(LHS).i32_const(33 - count);
    match kind {
        RotateKind::Left => {
            code.i32_shr_u();
        }
        RotateKind::Right => {
            code.i32_shl();
        }
    }
    let (carry_in_shift, carry_out_shift) = match kind {
        RotateKind::Left => (count - 1, 32 - count),
        RotateKind::Right => (32 - count, count - 1),
    };
    code.i32_or()
        .local_get(RHS)
        .i32_const(carry_in_shift)
        .i32_shl()
        .i32_or()
        .local_set(RESULT)
        .local_get(FLAGS)
        .i32_const(!0x801)
        .i32_and()
        .local_get(LHS)
        .i32_const(carry_out_shift)
        .i32_shr_u()
        .i32_const(1)
        .i32_and()
        .i32_or();
    // store validation clobbers scratch locals; publish flags only after success.
    memory::store_result(code, address, imports, exit_depth);
    code.local_set(FLAGS);
}

fn memory_rotate_through_carry_cl(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
    address: EffectiveAddress,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::load_result(code, address, imports, exit_depth);
    code.local_get(RESULT).local_set(LHS);
    shift_count(code, ShiftCount::Cl);
    code.local_tee(RHS)
        .if_(BlockType::Result(ValType::I32))
        .local_get(RHS)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Result(ValType::I32))
        .local_get(FLAGS)
        .i32_const(1)
        .i32_and()
        .local_set(RHS);
    rotate_through_carry_one_value(code, kind);
    code.local_get(FLAGS)
        .i32_const(!0x801)
        .i32_and()
        .local_get(LHS);
    match kind {
        RotateKind::Left => {
            code.i32_const(31).i32_shr_u();
        }
        RotateKind::Right => {
            code.i32_const(1).i32_and();
        }
    }
    code.i32_or();
    match kind {
        RotateKind::Left => {
            code.local_get(RESULT)
                .i32_const(31)
                .i32_shr_u()
                .local_get(LHS)
                .i32_const(31)
                .i32_shr_u();
        }
        RotateKind::Right => {
            code.local_get(LHS).i32_const(31).i32_shr_u().local_get(RHS);
        }
    }
    code.i32_xor()
        .i32_const(11)
        .i32_shl()
        .i32_or()
        .else_()
        .local_get(FLAGS)
        .i32_const(1)
        .i32_and()
        .local_set(RESULT)
        .local_get(LHS)
        .local_get(RHS);
    match kind {
        RotateKind::Left => {
            code.i32_shl();
        }
        RotateKind::Right => {
            code.i32_shr_u();
        }
    }
    code.local_get(LHS).i32_const(33).local_get(RHS).i32_sub();
    match kind {
        RotateKind::Left => {
            code.i32_shr_u();
        }
        RotateKind::Right => {
            code.i32_shl();
        }
    }
    code.i32_or().local_get(RESULT);
    match kind {
        RotateKind::Left => {
            code.local_get(RHS).i32_const(1).i32_sub();
        }
        RotateKind::Right => {
            code.i32_const(32).local_get(RHS).i32_sub();
        }
    }
    code.i32_shl()
        .i32_or()
        .local_set(RESULT)
        .local_get(FLAGS)
        .i32_const(!0x801)
        .i32_and()
        .local_get(LHS);
    match kind {
        RotateKind::Left => {
            code.i32_const(32).local_get(RHS).i32_sub();
        }
        RotateKind::Right => {
            code.local_get(RHS).i32_const(1).i32_sub();
        }
    }
    code.i32_shr_u()
        .i32_const(1)
        .i32_and()
        .i32_or()
        .end()
        .else_()
        .local_get(FLAGS)
        .end();
    // candidate flags survives store scratch and publishes only after success.
    memory::store_result(code, address, imports, exit_depth);
    code.local_set(FLAGS);
}

fn rotate_through_carry_cl(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
    destination: Register32,
) {
    shift_count(code, ShiftCount::Cl);
    code.local_tee(RHS)
        .if_(BlockType::Empty)
        .local_get(RHS)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Empty);
    rotate_through_carry_one(code, kind, destination);
    code.else_()
        .local_get(register(destination))
        .local_set(LHS)
        .local_get(FLAGS)
        .i32_const(1)
        .i32_and()
        .local_set(RESULT)
        .local_get(LHS)
        .local_get(RHS);
    match kind {
        RotateKind::Left => {
            code.i32_shl();
        }
        RotateKind::Right => {
            code.i32_shr_u();
        }
    }
    code.local_get(LHS).i32_const(33).local_get(RHS).i32_sub();
    match kind {
        RotateKind::Left => {
            code.i32_shr_u();
        }
        RotateKind::Right => {
            code.i32_shl();
        }
    }
    code.i32_or().local_get(RESULT);
    match kind {
        RotateKind::Left => {
            code.local_get(RHS).i32_const(1).i32_sub();
        }
        RotateKind::Right => {
            code.i32_const(32).local_get(RHS).i32_sub();
        }
    }
    code.i32_shl()
        .i32_or()
        .local_set(RESULT)
        .local_get(FLAGS)
        .i32_const(!0x801)
        .i32_and()
        .local_get(LHS);
    match kind {
        RotateKind::Left => {
            code.i32_const(32).local_get(RHS).i32_sub();
        }
        RotateKind::Right => {
            code.local_get(RHS).i32_const(1).i32_sub();
        }
    }
    code.i32_shr_u()
        .i32_const(1)
        .i32_and()
        .i32_or()
        .local_set(FLAGS)
        .local_get(RESULT)
        .local_set(register(destination))
        .end()
        .end();
}

fn rotate_through_carry_one(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
    destination: Register32,
) {
    code.local_get(register(destination))
        .local_set(LHS)
        .local_get(FLAGS)
        .i32_const(1)
        .i32_and()
        .local_set(RHS);
    rotate_through_carry_one_value(code, kind);
    code.local_get(RESULT).local_set(register(destination));
    rotate_through_carry_one_flags(code, kind);
}

fn rotate_through_carry_one_value(code: &mut InstructionSink<'_>, kind: RotateKind) {
    code.local_get(LHS).i32_const(1);
    match kind {
        RotateKind::Left => {
            code.i32_shl();
        }
        RotateKind::Right => {
            code.i32_shr_u();
        }
    }
    code.local_get(RHS);
    if kind == RotateKind::Right {
        code.i32_const(31).i32_shl();
    }
    code.i32_or().local_set(RESULT);
}

fn rotate_through_carry_one_flags(code: &mut InstructionSink<'_>, kind: RotateKind) {
    code.local_get(FLAGS)
        .i32_const(!0x801)
        .i32_and()
        .local_get(LHS);
    match kind {
        RotateKind::Left => {
            code.i32_const(31).i32_shr_u();
        }
        RotateKind::Right => {
            code.i32_const(1).i32_and();
        }
    }
    code.i32_or();
    match kind {
        RotateKind::Left => {
            code.local_get(RESULT)
                .i32_const(31)
                .i32_shr_u()
                .local_get(LHS)
                .i32_const(31)
                .i32_shr_u();
        }
        RotateKind::Right => {
            code.local_get(LHS).i32_const(31).i32_shr_u().local_get(RHS);
        }
    }
    code.i32_xor()
        .i32_const(11)
        .i32_shl()
        .i32_or()
        .local_set(FLAGS);
}

fn bit_scan(
    code: &mut InstructionSink<'_>,
    kind: BitScanKind,
    destination: Register32,
    source: u32,
) {
    code.local_get(source)
        .local_set(LHS)
        .local_get(LHS)
        .if_(BlockType::Empty);
    match kind {
        BitScanKind::Forward => {
            code.local_get(LHS).i32_ctz();
        }
        BitScanKind::Reverse => {
            code.i32_const(31).local_get(LHS).i32_clz().i32_sub();
        }
    }
    code.local_set(register(destination))
        .end()
        .local_get(FLAGS)
        .i32_const(0x402)
        .i32_and()
        .local_get(LHS)
        .i32_eqz()
        .i32_const(6)
        .i32_shl()
        .i32_or()
        .local_get(LHS)
        .i32_popcnt()
        .i32_const(1)
        .i32_and()
        .i32_eqz()
        .i32_const(2)
        .i32_shl()
        .i32_or()
        .local_set(FLAGS);
}

fn memory_bit_mutation(
    code: &mut InstructionSink<'_>,
    kind: BitTestKind,
    address: EffectiveAddress,
    index: BitIndex,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::address_value(code, address);
    if let BitIndex::Register(source) = index {
        code.local_get(register(source))
            .i32_const(5)
            .i32_shr_s()
            .i32_const(2)
            .i32_shl()
            .i32_add();
    }
    code.local_set(ADDRESS);
    match index {
        BitIndex::Register(source) => {
            code.local_get(register(source));
        }
        BitIndex::Immediate(raw) => {
            code.i32_const(i32::from(raw));
        }
    }
    code.i32_const(31).i32_and();
    memory::load_result_at_address(code, imports, exit_depth);
    code.local_set(RHS)
        .local_get(FLAGS)
        .i32_const(0x442)
        .i32_and()
        .local_get(RESULT)
        .local_get(RHS)
        .i32_shr_u()
        .i32_const(1)
        .i32_and()
        .i32_or();
    code.local_get(RESULT).i32_const(1).local_get(RHS).i32_shl();
    match kind {
        BitTestKind::Set => {
            code.i32_or();
        }
        BitTestKind::Reset => {
            code.i32_const(-1).i32_xor().i32_and();
        }
        BitTestKind::Complement => {
            code.i32_xor();
        }
        BitTestKind::Test => unreachable!("read-only BT has a separate operation"),
    }
    code.local_set(RESULT);
    // candidate flags stay below the checked store's scratch operands.
    memory::store_result_at_address(code, imports, exit_depth);
    code.local_set(FLAGS);
}

fn bit_test(
    code: &mut InstructionSink<'_>,
    kind: BitTestKind,
    destination: Register32,
    index: BitIndex,
) {
    code.local_get(register(destination)).local_set(LHS);
    match index {
        BitIndex::Register(source) => {
            code.local_get(register(source));
        }
        BitIndex::Immediate(raw) => {
            code.i32_const(i32::from(raw));
        }
    }
    code.i32_const(31)
        .i32_and()
        .local_set(RHS)
        .local_get(FLAGS)
        .i32_const(0x442)
        .i32_and()
        .local_get(LHS)
        .local_get(RHS)
        .i32_shr_u()
        .i32_const(1)
        .i32_and()
        .i32_or()
        .local_set(FLAGS);
    if kind != BitTestKind::Test {
        code.local_get(LHS).i32_const(1).local_get(RHS).i32_shl();
        match kind {
            BitTestKind::Set => {
                code.i32_or();
            }
            BitTestKind::Reset => {
                code.i32_const(-1).i32_xor().i32_and();
            }
            BitTestKind::Complement => {
                code.i32_xor();
            }
            BitTestKind::Test => unreachable!(),
        }
        code.local_set(register(destination));
    }
}

fn byte_accumulator_multiply(
    code: &mut InstructionSink<'_>,
    kind: MultiplyKind,
    source: ByteRegister,
) {
    byte_value(code, ByteValue::Register(ByteRegister::Al));
    code.local_set(LHS);
    byte_value(code, ByteValue::Register(source));
    code.local_set(RHS);
    byte_accumulator_multiply_value(code, kind);
}

fn byte_accumulator_multiply_value(code: &mut InstructionSink<'_>, kind: MultiplyKind) {
    for operand in [LHS, RHS] {
        code.local_get(operand);
        if kind == MultiplyKind::Signed {
            extend_value(code, ExtensionKind::Sign, SmallWidth::Byte);
        }
    }
    code.i32_mul()
        .local_set(RESULT)
        .local_get(register(Register32::Eax))
        .i32_const(!0xffff)
        .i32_and()
        .local_get(RESULT)
        .i32_const(0xffff)
        .i32_and()
        .i32_or()
        .local_set(register(Register32::Eax))
        .local_get(FLAGS)
        .i32_const(0x402)
        .i32_and()
        .local_get(RESULT);
    match kind {
        MultiplyKind::Unsigned => {
            code.i32_const(0xff).i32_gt_u();
        }
        MultiplyKind::Signed => {
            code.local_get(RESULT);
            extend_value(code, ExtensionKind::Sign, SmallWidth::Byte);
            code.i32_ne();
        }
    }
    code.i32_const(0x801).i32_mul().i32_or().local_set(FLAGS);
}

fn accumulator_multiply(code: &mut InstructionSink<'_>, kind: MultiplyKind, source: u32) {
    code.local_get(register(Register32::Eax))
        .local_set(LHS)
        .local_get(source)
        .local_set(RHS)
        .local_get(LHS)
        .local_get(RHS)
        .i32_mul()
        .local_set(register(Register32::Eax));
    for operand in [LHS, RHS] {
        code.local_get(operand);
        match kind {
            MultiplyKind::Unsigned => {
                code.i64_extend_i32_u();
            }
            MultiplyKind::Signed => {
                code.i64_extend_i32_s();
            }
        }
    }
    code.i64_mul()
        .i64_const(32)
        .i64_shr_u()
        .i32_wrap_i64()
        .local_set(register(Register32::Edx))
        .local_get(FLAGS)
        .i32_const(0x402)
        .i32_and()
        .local_get(register(Register32::Edx));
    match kind {
        MultiplyKind::Unsigned => {
            code.i32_const(0);
        }
        MultiplyKind::Signed => {
            code.local_get(register(Register32::Eax))
                .i32_const(31)
                .i32_shr_s();
        }
    }
    code.i32_ne()
        .i32_const(0x801)
        .i32_mul()
        .i32_or()
        .local_set(FLAGS);
}

fn byte_accumulator_divide(
    code: &mut InstructionSink<'_>,
    kind: DivideKind,
    source: ByteRegister,
    exit_depth: u32,
) {
    byte_value(code, ByteValue::Register(source));
    byte_accumulator_divide_value(code, kind, exit_depth);
}

fn byte_accumulator_divide_value(
    code: &mut InstructionSink<'_>,
    kind: DivideKind,
    exit_depth: u32,
) {
    if kind == DivideKind::Signed {
        extend_value(code, ExtensionKind::Sign, SmallWidth::Byte);
    }
    code.local_set(RHS).local_get(register(Register32::Eax));
    extend_value(
        code,
        if kind == DivideKind::Signed {
            ExtensionKind::Sign
        } else {
            ExtensionKind::Zero
        },
        SmallWidth::Word,
    );
    code.local_set(LHS).local_get(RHS).i32_eqz();
    divide_error_if(code, exit_depth);
    code.local_get(LHS).local_get(RHS);
    match kind {
        DivideKind::Unsigned => {
            code.i32_div_u();
        }
        DivideKind::Signed => {
            code.i32_div_s();
        }
    }
    code.local_set(RESULT).local_get(RESULT);
    match kind {
        DivideKind::Unsigned => {
            code.i32_const(0xff).i32_gt_u();
        }
        DivideKind::Signed => {
            code.i32_const(-128)
                .i32_lt_s()
                .local_get(RESULT)
                .i32_const(127)
                .i32_gt_s()
                .i32_or();
        }
    }
    divide_error_if(code, exit_depth);
    code.local_get(LHS)
        .local_get(RESULT)
        .local_get(RHS)
        .i32_mul()
        .i32_sub()
        .local_set(RHS)
        .local_get(register(Register32::Eax))
        .i32_const(!0xffff)
        .i32_and()
        .local_get(RESULT)
        .i32_const(0xff)
        .i32_and()
        .i32_or()
        .local_get(RHS)
        .i32_const(0xff)
        .i32_and()
        .i32_const(8)
        .i32_shl()
        .i32_or()
        .local_set(register(Register32::Eax));
}

fn accumulator_divide(
    code: &mut InstructionSink<'_>,
    kind: DivideKind,
    source: u32,
    exit_depth: u32,
) {
    code.local_get(source)
        .local_set(RHS)
        .local_get(RHS)
        .i32_eqz();
    divide_error_if(code, exit_depth);
    match kind {
        DivideKind::Unsigned => {
            code.local_get(register(Register32::Edx))
                .local_get(RHS)
                .i32_ge_u();
        }
        DivideKind::Signed => {
            code.local_get(register(Register32::Edx))
                .i32_const(i32::MIN)
                .i32_eq()
                .local_get(register(Register32::Eax))
                .i32_eqz()
                .i32_and()
                .local_get(RHS)
                .i32_const(-1)
                .i32_eq()
                .i32_and();
        }
    }
    divide_error_if(code, exit_depth);
    divide_operands(code, kind);
    match kind {
        DivideKind::Unsigned => {
            code.i64_div_u();
        }
        DivideKind::Signed => {
            code.i64_div_s();
        }
    }
    // preflight has finished using this i64 local; keep the quotient wide until validation.
    code.local_set(MEMORY_BYTES);
    if kind == DivideKind::Signed {
        code.local_get(MEMORY_BYTES)
            .i64_const(i64::from(i32::MIN))
            .i64_lt_s()
            .local_get(MEMORY_BYTES)
            .i64_const(i64::from(i32::MAX))
            .i64_gt_s()
            .i32_or();
        divide_error_if(code, exit_depth);
    }
    divide_operands(code, kind);
    code.local_get(MEMORY_BYTES)
        .i64_mul()
        .i64_sub()
        .i32_wrap_i64()
        .local_set(RESULT)
        .local_get(MEMORY_BYTES)
        .i32_wrap_i64()
        .local_set(register(Register32::Eax))
        .local_get(RESULT)
        .local_set(register(Register32::Edx));
}

fn divide_operands(code: &mut InstructionSink<'_>, kind: DivideKind) {
    code.local_get(register(Register32::Edx))
        .i64_extend_i32_u()
        .i64_const(32)
        .i64_shl()
        .local_get(register(Register32::Eax))
        .i64_extend_i32_u()
        .i64_or()
        .local_get(RHS);
    match kind {
        DivideKind::Unsigned => {
            code.i64_extend_i32_u();
        }
        DivideKind::Signed => {
            code.i64_extend_i32_s();
        }
    }
}

fn divide_error_if(code: &mut InstructionSink<'_>, exit_depth: u32) {
    code.if_(BlockType::Empty)
        .i32_const(10)
        .local_set(REASON)
        .br(exit_depth + 1)
        .end();
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
