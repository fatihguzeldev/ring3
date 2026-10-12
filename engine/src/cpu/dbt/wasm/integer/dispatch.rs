use super::super::{atomic, integer_system, segments, simd, stack, strings};
use super::{
    ADDRESS, BUDGET, BinaryKind, BitIndex, BitTestKind, BlockType, BranchTarget,
    ByteArithmeticKind, ByteLogicalKind, BytePredicateKind, ByteReadArithmeticKind, ByteRegister,
    ByteValue, CarryFlag, CarryKind, Condition, CountBranchKind, DecodedInstruction, EIP,
    EffectiveAddress, ExtensionKind, FLAGS, InstructionSink, LHS, Location32, Operation, RESULT,
    RETIRED, RHS, Register32, ShiftCount, SmallSource, SmallWidth, ValType, Value32,
    WordArithmeticKind, WordLogicalKind, WordMemoryArithmeticKind, WordReadArithmeticKind,
    WordValue, accumulator_divide, accumulator_multiply, arithmetic_flags, arithmetic_memory_byte,
    ascii_adjust, ascii_adjust_after_multiplication, ascii_adjust_before_division,
    binary_memory_store, bit_scan, bit_scan_word, bit_test, bit_test_word, byte_accumulator_divide,
    byte_accumulator_divide_value, byte_accumulator_multiply, byte_accumulator_multiply_value,
    byte_rotate_cl, byte_rotate_immediate, byte_rotate_memory, byte_rotate_one,
    byte_rotate_one_flags, byte_rotate_one_value, byte_rotate_through_carry,
    byte_rotate_through_carry_cl, byte_rotate_through_carry_memory_cl,
    byte_rotate_through_carry_memory_immediate, byte_rotate_through_carry_one,
    byte_rotate_through_carry_one_flags, byte_rotate_through_carry_one_value, byte_value, control,
    decimal_adjust, double_shift, double_shift_memory, double_shift_memory_word, double_shift_word,
    extend_byte_to_word, extend_value, indirect_target, insert_byte, logical_flags,
    logical_memory_byte, memory, memory_bit_mutation, memory_bit_test_word,
    memory_rotate_through_carry_cl, memory_rotate_through_carry_immediate, register, rotate,
    rotate_memory, rotate_memory_word, rotate_one, rotate_one_flags, rotate_one_value,
    rotate_through_carry_cl, rotate_through_carry_immediate, rotate_through_carry_memory_word,
    rotate_through_carry_one, rotate_through_carry_one_flags, rotate_through_carry_one_value,
    rotate_through_carry_word, rotate_word, shift, shift_byte, shift_memory, shift_memory_byte,
    shift_memory_byte_cl, shift_memory_byte_immediate, shift_memory_word, shift_word,
    signed_multiply, signed_multiply_word, trailing_zero_count32, unary, unary_byte, unary_memory,
    unary_memory_byte, unary_memory_word, unary_word, value, word_accumulator_divide,
    word_accumulator_multiply, word_value, x87,
};

pub(in crate::cpu::dbt::wasm) fn instruction(
    code: &mut InstructionSink<'_>,
    instruction: &DecodedInstruction,
    imports: memory::Imports,
    exit_depth: u32,
) {
    if !matches!(
        instruction.operation(),
        Operation::String { .. } | Operation::RepeatString { .. }
    ) && !matches!(instruction.operation(), Operation::Simd(operation) if operation.kind == crate::cpu::x86::simd::SimdKind::MaskedStore)
    {
        use crate::cpu::x86::SegmentRegister::*;
        for segment in [Es, Cs, Ss, Ds, Fs, Gs] {
            if instruction.segment_accesses() & (1 << segment.index()) != 0 {
                segments::access(code, segment, exit_depth);
            }
        }
    }
    let mut store = false;
    match *instruction.operation() {
        Operation::XState(_) => {
            store = super::super::xstate::instruction(code, instruction, imports, exit_depth);
        }
        Operation::AdjustPrivilege {
            destination,
            source,
        } => {
            store = super::super::segment_queries::adjust_privilege(
                code,
                destination,
                source,
                imports,
                exit_depth,
            );
        }
        Operation::CacheFlush { address } => {
            super::super::cache_flush::instruction(code, address, imports, exit_depth);
        }
        Operation::VirtualTimestampAux => {
            super::super::integer_system::timestamp(code, true, imports, exit_depth);
        }
        Operation::PackedStringCompare(operation) => {
            store = super::super::packed_strings::instruction(code, operation, imports, exit_depth);
        }
        Operation::Crc32 {
            destination,
            source,
        } => {
            super::scalar_bits::crc32(code, destination, source, imports, exit_depth);
        }
        Operation::MultiplyExtended { high, low, source } => {
            super::bit_manipulation::multiply_extended(
                code, high, low, source, imports, exit_depth,
            );
        }
        Operation::BitManipulation {
            kind,
            destination,
            source,
        } => {
            super::bit_manipulation::bit_manipulation(
                code,
                kind,
                destination,
                source,
                imports,
                exit_depth,
            );
        }
        Operation::Control(_) => {
            super::super::control_instructions::instruction(code, instruction, imports, exit_depth);
            return;
        }
        Operation::BitCount {
            kind,
            width,
            destination,
            source,
        } => {
            super::scalar_bits::bit_count(
                code,
                kind,
                width,
                destination,
                source,
                imports,
                exit_depth,
            );
        }
        Operation::MoveByteSwap {
            width,
            destination,
            source,
        } => {
            store = super::scalar_bits::move_byte_swap(
                code,
                width,
                destination,
                source,
                imports,
                exit_depth,
            );
        }
        Operation::StackFrame { .. }
        | Operation::PopAll
        | Operation::PopAllWord
        | Operation::EnterZero
        | Operation::EnterZeroWord => {
            stack::instruction(code, instruction, imports, exit_depth);
            return;
        }
        Operation::RepeatString { .. } => {
            strings::instruction(code, instruction, imports, exit_depth);
            return;
        }
        Operation::ReadSegment { .. }
        | Operation::PushSegment { .. }
        | Operation::LoadSegment { .. }
        | Operation::PopSegment { .. }
        | Operation::LoadFarPointer { .. } => {
            segments::instruction(code, instruction, imports, exit_depth);
            return;
        }
        Operation::X87 {
            operation,
            address,
            waiting,
            opcode,
        } => {
            x87::operation(
                code,
                operation,
                address,
                waiting,
                instruction.pc().0,
                opcode,
                imports.x87_op.expect("x87 execution import"),
                exit_depth,
            );
            store = operation.write_size() != 0;
        }
        Operation::Simd(operation) => {
            store = simd::instruction(code, operation, imports, exit_depth)
        }
        Operation::String { .. } => {
            strings::instruction(code, instruction, imports, exit_depth);
            return;
        }
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
        Operation::SignExtendAccumulatorWord => {
            code.local_get(register(Register32::Eax))
                .i32_const(!0xffff)
                .i32_and()
                .local_get(register(Register32::Eax));
            extend_value(code, ExtensionKind::Sign, SmallWidth::Byte);
            code.i32_const(0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(Register32::Eax));
        }
        Operation::SignExtendHighWord => {
            code.local_get(register(Register32::Edx))
                .i32_const(!0xffff)
                .i32_and()
                .local_get(register(Register32::Eax))
                .i32_const(16)
                .i32_shl()
                .i32_const(31)
                .i32_shr_s()
                .i32_const(0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(Register32::Edx));
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
        Operation::DecimalAdjust { kind } => decimal_adjust(code, kind),
        Operation::AsciiAdjust { kind } => ascii_adjust(code, kind),
        Operation::AsciiAdjustBeforeDivision { base } => ascii_adjust_before_division(code, base),
        Operation::AsciiAdjustAfterMultiplication { base } => {
            ascii_adjust_after_multiplication(code, base, exit_depth);
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
        Operation::TranslateByteAddress { address } => {
            code.local_get(register(Register32::Ebx))
                .local_get(register(Register32::Eax))
                .i32_const(255)
                .i32_and()
                .i32_add();
            if address.address_size == crate::cpu::x86::ir::AddressSize::Bits16 {
                code.i32_const(65535).i32_and();
            }
            memory::checked_offset(
                code,
                address.resolved_segment(),
                1,
                crate::cpu::x86::SegmentAccess::Read,
                imports,
                exit_depth,
            );
            memory::load_narrow_at_address(code, SmallWidth::Byte, imports, exit_depth);
            insert_byte(code, ByteRegister::Al);
        }
        Operation::CpuId => integer_system::cpuid(code),
        Operation::VirtualTimestamp => integer_system::timestamp(code, false, imports, exit_depth),
        Operation::AtomicMemory {
            kind,
            width,
            address,
            source,
        } => {
            atomic::operation(code, kind, width, address, source, imports, exit_depth);
            store = true;
        }
        Operation::CompareExchange8B { address } => {
            atomic::compare8(code, address, imports, exit_depth);
            store = true;
        }
        Operation::TranslateByte => {
            code.local_get(register(Register32::Ebx))
                .local_get(register(Register32::Eax))
                .i32_const(0xff)
                .i32_and()
                .i32_add();
            memory::checked_offset(
                code,
                crate::cpu::x86::SegmentRegister::Ds,
                1,
                crate::cpu::x86::SegmentAccess::Read,
                imports,
                exit_depth,
            );
            memory::load_narrow_at_address(code, SmallWidth::Byte, imports, exit_depth);
            insert_byte(code, ByteRegister::Al);
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
            memory::load_narrow_for_modify(code, address, SmallWidth::Word, imports, exit_depth);
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
            // the modify read retains ADDRESS; the source cannot access memory.
            memory::store_word_result_at_address(code, imports, exit_depth);
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
            memory::load_narrow_for_modify(code, address, SmallWidth::Word, imports, exit_depth);
            // helper validation uses operand scratch; capture the source after success.
            word_value(code, source);
            match kind {
                WordLogicalKind::And => code.i32_and(),
                WordLogicalKind::Or => code.i32_or(),
                WordLogicalKind::Xor => code.i32_xor(),
            };
            code.local_set(RESULT);
            // the modify read retains ADDRESS; the source cannot access memory.
            memory::store_word_result_at_address(code, imports, exit_depth);
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
        Operation::ExtendByteToWord {
            kind,
            destination,
            source,
        } => {
            byte_value(code, ByteValue::Register(source));
            extend_byte_to_word(code, kind, destination);
        }
        Operation::ReadExtendByteToWord {
            kind,
            destination,
            address,
        } => {
            memory::load_narrow_value(code, address, SmallWidth::Byte, imports, exit_depth);
            extend_byte_to_word(code, kind, destination);
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
        Operation::ExchangeWord { left, right } => {
            code.local_get(register(left))
                .i32_const(!0xffff)
                .i32_and()
                .local_get(register(right))
                .i32_const(0xffff)
                .i32_and()
                .i32_or()
                .local_get(register(right))
                .i32_const(!0xffff)
                .i32_and()
                .local_get(register(left))
                .i32_const(0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(right))
                .local_set(register(left));
        }
        Operation::Exchange { left, right } => {
            code.local_get(register(left))
                .local_get(register(right))
                .local_set(register(left))
                .local_set(register(right));
        }
        Operation::MemoryExchange { address, source } => {
            memory::exchange_result(code, address, source, imports, exit_depth);
            code.local_get(RESULT).local_set(register(source));
            store = true;
        }
        Operation::MemoryExchangeByte { address, source } => {
            byte_value(code, ByteValue::Register(source));
            code.local_set(RESULT);
            memory::exchange_byte_result(code, address, imports, exit_depth);
            code.local_get(RESULT);
            insert_byte(code, source);
            store = true;
        }
        Operation::MemoryExchangeWord { address, source } => {
            word_value(code, WordValue::Register(source));
            code.local_set(RESULT);
            memory::exchange_word_result(code, address, imports, exit_depth);
            code.local_get(RESULT)
                .local_get(register(source))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(source));
            store = true;
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
        Operation::MemoryExchangeAddWord { address, source } => {
            memory::load_narrow_for_modify(code, address, SmallWidth::Word, imports, exit_depth);
            code.local_set(LHS);
            word_value(code, WordValue::Register(source));
            code.local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_add()
                .i32_const(0xffff)
                .i32_and()
                .local_set(RESULT)
                .local_get(LHS)
                .local_get(RHS);
            memory::store_word_result(code, address, imports, exit_depth);
            code.local_set(RHS).local_set(LHS);
            arithmetic_flags(code, BinaryKind::Add, CarryFlag::Calculate, 15);
            code.local_get(LHS)
                .local_get(register(source))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(source));
            store = true;
        }
        Operation::MemoryExchangeAdd { address, source } => {
            memory::load_modify_result(code, address, imports, exit_depth);
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
        Operation::LockedMemoryExchangeAdd32 { address, source } => {
            memory::exchange_add_result(code, address, source, imports, exit_depth);
            code.local_get(RESULT)
                .local_set(LHS)
                .local_get(register(source))
                .local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_add()
                .local_set(RESULT);
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
        Operation::LockedMemoryExchangeAddByte { address, source } => {
            byte_value(code, ByteValue::Register(source));
            code.local_set(RESULT);
            memory::exchange_add_byte_result(code, address, imports, exit_depth);
            code.local_get(RESULT).local_set(LHS);
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
            store = true;
        }
        Operation::LockedMemoryExchangeAddWord { address, source } => {
            word_value(code, WordValue::Register(source));
            code.local_set(RESULT);
            memory::exchange_add_word_result(code, address, imports, exit_depth);
            code.local_get(RESULT).local_set(LHS);
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
                .local_set(register(source));
            store = true;
        }
        Operation::MemoryExchangeAddByte { address, source } => {
            memory::load_narrow_for_modify(code, address, SmallWidth::Byte, imports, exit_depth);
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
        Operation::LockedMemoryCompareExchangeWord { address, source } => {
            word_value(code, WordValue::Register(Register32::Eax));
            word_value(code, WordValue::Register(source));
            code.local_set(RESULT);
            memory::compare_exchange_word_result(code, address, imports, exit_depth);
            code.local_set(LHS)
                .local_get(RESULT)
                .local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_sub()
                .i32_const(0xffff)
                .i32_and()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 15);
            code.local_get(RESULT)
                .if_(BlockType::Empty)
                .local_get(RHS)
                .local_get(register(Register32::Eax))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(Register32::Eax))
                .end();
            store = true;
        }
        Operation::MemoryCompareExchangeWord { address, source } => {
            memory::load_narrow_for_modify(code, address, SmallWidth::Word, imports, exit_depth);
            code.local_set(RHS);
            word_value(code, WordValue::Register(Register32::Eax));
            code.local_set(LHS);
            word_value(code, WordValue::Register(source));
            code.local_get(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_eq()
                .select()
                .local_set(RESULT)
                .local_get(LHS)
                .local_get(RHS);
            memory::store_word_result(code, address, imports, exit_depth);
            code.local_set(RHS)
                .local_set(LHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_sub()
                .i32_const(0xffff)
                .i32_and()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 15);
            code.local_get(RESULT)
                .if_(BlockType::Empty)
                .local_get(RHS)
                .local_get(register(Register32::Eax))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(Register32::Eax))
                .end();
            store = true;
        }
        Operation::LockedMemoryCompareExchange32 { address, source } => {
            code.local_get(register(Register32::Eax));
            memory::compare_exchange_result(code, address, source, imports, exit_depth);
            code.local_set(LHS)
                .local_get(RESULT)
                .local_set(RHS)
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
        Operation::MemoryCompareExchange { address, source } => {
            memory::load_modify_result(code, address, imports, exit_depth);
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
        Operation::LockedMemoryCompareExchangeByte { address, source } => {
            byte_value(code, ByteValue::Register(ByteRegister::Al));
            byte_value(code, ByteValue::Register(source));
            code.local_set(RESULT);
            memory::compare_exchange_byte_result(code, address, imports, exit_depth);
            code.local_set(LHS)
                .local_get(RESULT)
                .local_set(RHS)
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
        Operation::MemoryCompareExchangeByte { address, source } => {
            memory::load_narrow_for_modify(code, address, SmallWidth::Byte, imports, exit_depth);
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
            memory::offset_value(code, address);
            code.local_set(register(destination));
        }
        Operation::LeaWord {
            destination,
            address,
        } => {
            memory::offset_value(code, address);
            code.i32_const(0xffff)
                .i32_and()
                .local_get(register(destination))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(destination));
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
                    if matches!(kind, BinaryKind::Cmp | BinaryKind::Test) {
                        memory::load_result(code, address, imports, exit_depth);
                    } else {
                        memory::load_modify_result(code, address, imports, exit_depth);
                    }
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
        Operation::MemoryUnaryWord { kind, address } => {
            unary_memory_word(code, kind, address, imports, exit_depth);
            store = true;
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
        Operation::RotateWord {
            kind,
            destination,
            count,
        } => rotate_word(code, kind, destination, count),
        Operation::MemoryRotateWord {
            kind,
            address,
            count,
        } => {
            rotate_memory_word(code, kind, address, count, imports, exit_depth);
            store = true;
        }
        Operation::RotateThroughCarryWord {
            kind,
            destination,
            count,
        } => rotate_through_carry_word(code, kind, destination, count),
        Operation::MemoryRotateThroughCarryWord {
            kind,
            address,
            count,
        } => {
            rotate_through_carry_memory_word(code, kind, address, count, imports, exit_depth);
            store = true;
        }
        Operation::ShiftWord {
            kind,
            destination,
            count,
        } => shift_word(code, kind, destination, count),
        Operation::MemoryShiftWord {
            kind,
            address,
            count,
        } => {
            shift_memory_word(code, kind, address, count, imports, exit_depth);
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
        Operation::MemoryDoubleShiftWord {
            kind,
            address,
            source,
            count,
        } => {
            double_shift_memory_word(code, kind, address, source, count, imports, exit_depth);
            store = true;
        }
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
        Operation::BitTestWord {
            kind,
            destination,
            index,
        } => bit_test_word(code, kind, destination, index),
        Operation::ReadBitTestWord { address, index } => {
            memory_bit_test_word(code, BitTestKind::Test, address, index, imports, exit_depth);
        }
        Operation::MemoryBitMutationWord {
            kind,
            address,
            index,
        } => {
            memory_bit_test_word(code, kind, address, index, imports, exit_depth);
            store = true;
        }
        Operation::LockedMemoryBitSet32 { address, index } => {
            memory::bit_test_set_result(code, address, index, imports, exit_depth);
            code.local_set(RHS)
                .local_get(FLAGS)
                .i32_const(!0x895)
                .i32_and()
                .local_get(RESULT)
                .local_get(RHS)
                .i32_shr_u()
                .i32_const(1)
                .i32_and()
                .i32_or()
                .local_set(FLAGS);
            store = true;
        }
        Operation::LockedMemoryBitReset32 { address, index } => {
            memory::bit_test_reset_result(code, address, index, imports, exit_depth);
            code.local_set(RHS)
                .local_get(FLAGS)
                .i32_const(!0x895)
                .i32_and()
                .local_get(RESULT)
                .local_get(RHS)
                .i32_shr_u()
                .i32_const(1)
                .i32_and()
                .i32_or()
                .local_set(FLAGS);
            store = true;
        }
        Operation::LockedMemoryOr32 { address, mask } => {
            memory::atomic_or_result(code, address, mask, imports, exit_depth);
            code.local_get(RESULT)
                .i32_const(mask as i32)
                .i32_or()
                .local_set(RESULT);
            logical_flags(code, 31);
            store = true;
        }
        Operation::LockedMemoryAnd32 { address, mask } => {
            memory::atomic_and_result(code, address, mask, imports, exit_depth);
            code.local_get(RESULT)
                .i32_const(mask as i32)
                .i32_and()
                .local_set(RESULT);
            logical_flags(code, 31);
            store = true;
        }
        Operation::LockedMemoryXor32 { address, mask } => {
            memory::atomic_xor_result(code, address, mask, imports, exit_depth);
            code.local_get(RESULT)
                .i32_const(mask as i32)
                .i32_xor()
                .local_set(RESULT);
            logical_flags(code, 31);
            store = true;
        }
        Operation::LockedMemoryOrRegister32 { address, source } => {
            memory::atomic_or_register_result(code, address, source, imports, exit_depth);
            code.local_get(RESULT)
                .local_get(register(source))
                .i32_or()
                .local_set(RESULT);
            logical_flags(code, 31);
            store = true;
        }
        Operation::LockedMemoryAndRegister32 { address, source } => {
            memory::atomic_and_register_result(code, address, source, imports, exit_depth);
            code.local_get(RESULT)
                .local_get(register(source))
                .i32_and()
                .local_set(RESULT);
            logical_flags(code, 31);
            store = true;
        }
        Operation::LockedMemoryXorRegister32 { address, source } => {
            memory::atomic_xor_register_result(code, address, source, imports, exit_depth);
            code.local_get(RESULT)
                .local_get(register(source))
                .i32_xor()
                .local_set(RESULT);
            logical_flags(code, 31);
            store = true;
        }
        Operation::MemoryBitMutation {
            kind,
            address,
            index,
        } => {
            memory_bit_mutation(code, kind, address, index, imports, exit_depth);
            store = true;
        }
        Operation::ReadBitTest { address, index } => {
            memory::offset_value(code, address);
            if let BitIndex::Register(source) = index {
                code.local_get(register(source))
                    .i32_const(5)
                    .i32_shr_s()
                    .i32_const(2)
                    .i32_shl()
                    .i32_add();
            }
            if address.address_size == crate::cpu::x86::ir::AddressSize::Bits16 {
                code.i32_const(0xffff).i32_and();
            }
            memory::checked_offset(
                code,
                address.resolved_segment(),
                4,
                crate::cpu::x86::SegmentAccess::Read,
                imports,
                exit_depth,
            );
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
                .i32_const(!0x895)
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
            memory::load_narrow_for_modify(code, address, SmallWidth::Byte, imports, exit_depth);
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
            memory::load_narrow_for_modify(code, address, SmallWidth::Byte, imports, exit_depth);
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
            memory::load_modify_result(code, address, imports, exit_depth);
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
            memory::load_modify_result(code, address, imports, exit_depth);
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
        Operation::SignedMultiplyWord {
            destination,
            source,
        } => {
            signed_multiply_word(code, destination, register(source), None);
        }
        Operation::ReadSignedMultiplyWord {
            destination,
            address,
        } => {
            memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
            code.local_set(RESULT);
            signed_multiply_word(code, destination, RESULT, None);
        }
        Operation::SignedMultiplyImmediateWord {
            destination,
            source,
            immediate,
        } => {
            signed_multiply_word(code, destination, register(source), Some(immediate));
        }
        Operation::ReadSignedMultiplyImmediateWord {
            destination,
            address,
            immediate,
        } => {
            memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
            code.local_set(RESULT);
            signed_multiply_word(code, destination, RESULT, Some(immediate));
        }
        Operation::MultiplyAccumulator { kind, source } => {
            accumulator_multiply(code, kind, register(source));
        }
        Operation::WordMultiplyAccumulator { kind, source } => {
            word_accumulator_multiply(code, kind, register(source));
        }
        Operation::ReadWordMultiplyAccumulator { kind, address } => {
            memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
            code.local_set(RESULT);
            word_accumulator_multiply(code, kind, RESULT);
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
        Operation::WordDivideAccumulator { kind, source } => {
            word_accumulator_divide(code, kind, register(source), exit_depth);
        }
        Operation::ReadWordDivideAccumulator { kind, address } => {
            memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
            code.local_set(RESULT);
            word_accumulator_divide(code, kind, RESULT, exit_depth);
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
        Operation::TrailingZeroCount32 {
            destination,
            source,
        } => {
            trailing_zero_count32(code, destination, register(source));
        }
        Operation::ReadTrailingZeroCount32 {
            destination,
            address,
        } => {
            memory::load_result(code, address, imports, exit_depth);
            trailing_zero_count32(code, destination, RESULT);
        }
        Operation::BitScan {
            kind,
            destination,
            source,
        } => {
            bit_scan(code, kind, destination, register(source));
        }
        Operation::BitScanWord {
            kind,
            destination,
            source,
        } => {
            bit_scan_word(code, kind, destination, register(source));
        }
        Operation::ReadBitScan {
            kind,
            destination,
            address,
        } => {
            memory::load_result(code, address, imports, exit_depth);
            bit_scan(code, kind, destination, RESULT);
        }
        Operation::ReadBitScanWord {
            kind,
            destination,
            address,
        } => {
            memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
            code.local_set(RESULT);
            bit_scan_word(code, kind, destination, RESULT);
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
        Operation::JumpWord { source } => {
            code.local_get(register(source))
                .i32_const(0xffff)
                .i32_and()
                .local_set(EIP);
        }
        Operation::ReadJumpWord { address } => {
            memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
            code.local_set(EIP);
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
        Operation::CountBranchWord { kind, target } => {
            let ecx = register(Register32::Ecx);
            if kind == CountBranchKind::EcxZero {
                code.local_get(ecx).i32_const(65535).i32_and().i32_eqz();
            } else {
                code.local_get(ecx)
                    .i32_const(-65536)
                    .i32_and()
                    .local_get(ecx)
                    .i32_const(1)
                    .i32_sub()
                    .i32_const(65535)
                    .i32_and()
                    .i32_or()
                    .local_tee(ecx)
                    .i32_const(65535)
                    .i32_and()
                    .i32_eqz()
                    .i32_eqz();
                if matches!(
                    kind,
                    CountBranchKind::LoopEqual | CountBranchKind::LoopNotEqual
                ) {
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
            }
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
        Operation::CallWord { target } => {
            code.i32_const((instruction.next_pc().0 & 0xffff) as i32)
                .local_set(RESULT);
            memory::push_word_result(code, imports, exit_depth);
            code.i32_const(target.0 as i32).local_set(EIP);
            store = true;
        }
        Operation::CallRegisterWord { source } => {
            word_value(code, WordValue::Register(source));
            code.i32_const((instruction.next_pc().0 & 0xffff) as i32)
                .local_set(RESULT);
            memory::push_word_result(code, imports, exit_depth);
            code.local_set(EIP);
            store = true;
        }
        Operation::ReadCallWord { address } => {
            memory::checked_address(
                code,
                address,
                2,
                crate::cpu::x86::SegmentAccess::Read,
                imports,
                exit_depth,
            );
            memory::checked_address(
                code,
                EffectiveAddress {
                    base: Some(Register32::Esp),
                    displacement: (-2_i32) as u32,
                    ..Default::default()
                },
                2,
                crate::cpu::x86::SegmentAccess::Write,
                imports,
                exit_depth,
            );
            memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
            code.i32_const((instruction.next_pc().0 & 0xffff) as i32)
                .local_set(RESULT);
            memory::push_word_result(code, imports, exit_depth);
            code.local_set(EIP);
            store = true;
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
                    ..Default::default()
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
            if let Location32::Memory(address) = target {
                memory::checked_address(
                    code,
                    address,
                    4,
                    crate::cpu::x86::SegmentAccess::Read,
                    imports,
                    exit_depth,
                );
                memory::checked_address(
                    code,
                    EffectiveAddress {
                        base: Some(Register32::Esp),
                        displacement: (-4_i32) as u32,
                        ..Default::default()
                    },
                    4,
                    crate::cpu::x86::SegmentAccess::Write,
                    imports,
                    exit_depth,
                );
            }
            indirect_target(code, target, imports, exit_depth);
            memory::store(
                code,
                EffectiveAddress {
                    base: Some(Register32::Esp),
                    index: None,
                    scale: 1,
                    displacement: (-4_i32) as u32,
                    ..Default::default()
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
                    ..Default::default()
                },
                source,
                imports,
                exit_depth,
            );
            code.local_get(ADDRESS).local_set(register(Register32::Esp));
            store = true;
        }
        Operation::PushWord { source } => {
            word_value(code, source);
            code.local_set(RESULT);
            memory::push_word_result(code, imports, exit_depth);
            store = true;
        }
        Operation::PushMemoryWord { source } => {
            memory::checked_address(
                code,
                source,
                2,
                crate::cpu::x86::SegmentAccess::Read,
                imports,
                exit_depth,
            );
            memory::checked_address(
                code,
                EffectiveAddress {
                    base: Some(Register32::Esp),
                    displacement: (-2_i32) as u32,
                    ..Default::default()
                },
                2,
                crate::cpu::x86::SegmentAccess::Write,
                imports,
                exit_depth,
            );
            memory::load_narrow_value(code, source, SmallWidth::Word, imports, exit_depth);
            code.local_set(RESULT);
            memory::push_word_result(code, imports, exit_depth);
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
                    ..Default::default()
                },
                imports,
                exit_depth,
            );
            code.local_get(ADDRESS).local_set(register(Register32::Esp));
            store = true;
        }
        Operation::PushFlagsWord => {
            code.local_get(FLAGS)
                .i32_const(0xffff)
                .i32_and()
                .local_set(RESULT);
            memory::push_word_result(code, imports, exit_depth);
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
        Operation::PopWord { destination } => {
            memory::pop_word_register(code, destination, imports, exit_depth);
        }
        Operation::PopMemoryWord { destination } => {
            memory::pop_word_memory(code, destination, imports, exit_depth);
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
                    ..Default::default()
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
        Operation::LeaveWord => {
            memory::load_narrow_value(
                code,
                EffectiveAddress {
                    base: Some(Register32::Ebp),
                    index: None,
                    scale: 1,
                    displacement: 0,
                    ..Default::default()
                },
                SmallWidth::Word,
                imports,
                exit_depth,
            );
            code.local_get(ADDRESS)
                .i32_const(2)
                .i32_add()
                .local_set(register(Register32::Esp))
                .local_get(register(Register32::Ebp))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(Register32::Ebp));
        }
        Operation::Return { stack_adjust } => {
            memory::pop_return(code, stack_adjust, imports, exit_depth);
        }
        Operation::ReturnWord { stack_adjust } => {
            memory::load_narrow_value(
                code,
                EffectiveAddress {
                    base: Some(Register32::Esp),
                    index: None,
                    scale: 1,
                    displacement: 0,
                    ..Default::default()
                },
                SmallWidth::Word,
                imports,
                exit_depth,
            );
            code.local_set(EIP)
                .local_get(register(Register32::Esp))
                .i32_const(2 + i32::from(stack_adjust))
                .i32_add()
                .local_set(register(Register32::Esp));
        }
    }
    if !matches!(
        instruction.operation(),
        Operation::Jump { .. }
            | Operation::JumpWord { .. }
            | Operation::ReadJumpWord { .. }
            | Operation::ConditionalJump { .. }
            | Operation::CountBranch { .. }
            | Operation::CountBranchWord { .. }
            | Operation::Call { .. }
            | Operation::CallWord { .. }
            | Operation::CallRegisterWord { .. }
            | Operation::ReadCallWord { .. }
            | Operation::Return { .. }
            | Operation::ReturnWord { .. }
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
