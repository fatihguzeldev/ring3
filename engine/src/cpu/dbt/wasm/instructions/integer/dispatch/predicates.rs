use super::*;

pub(super) fn emit(
    code: &mut InstructionSink<'_>,
    instruction: &DecodedOperation,
    imports: Imports,
    exit_depth: u32,
) -> bool {
    let mut store = false;
    match *instruction.operation() {
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
            conditions::condition(code, condition);
            insert_byte(code, destination);
        }
        Operation::ConditionalMove {
            condition,
            destination,
            source,
        } => {
            code.local_get(register(source));
            code.local_get(register(destination));
            conditions::condition(code, condition);
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
            conditions::condition(code, condition);
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
            conditions::condition(code, condition);
            code.select().local_set(register(destination));
        }
        Operation::ReadConditionalMoveWord {
            condition,
            destination,
            address,
        } => {
            memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
            word_value(code, WordValue::Register(destination));
            conditions::condition(code, condition);
            code.select()
                .local_get(register(destination))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(destination));
        }
        Operation::MemorySetByte { condition, address } => {
            conditions::condition(code, condition);
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
        _ => unreachable!("instruction routed to predicates emitter"),
    }
    store
}
