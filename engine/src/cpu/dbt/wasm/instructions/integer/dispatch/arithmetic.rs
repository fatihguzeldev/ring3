use super::*;

#[cfg(test)]
mod tests;

pub(super) fn emit(
    code: &mut InstructionSink<'_>,
    instruction: &DecodedOperation,
    imports: Imports,
    exit_depth: u32,
) -> bool {
    let mut store = false;
    match *instruction.operation() {
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
        Operation::DecimalAdjust { kind } => decimal_adjust(code, kind),
        Operation::AsciiAdjust { kind } => ascii_adjust(code, kind),
        Operation::AsciiAdjustBeforeDivision { base } => ascii_adjust_before_division(code, base),
        Operation::AsciiAdjustAfterMultiplication { base } => {
            ascii_adjust_after_multiplication(code, base, exit_depth);
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
        Operation::Binary {
            kind: kind @ (BinaryKind::Xor | BinaryKind::Sub | BinaryKind::Cmp),
            destination: Location32::Register(destination),
            source: Value32::Register(source),
        } if destination == source => {
            if kind != BinaryKind::Cmp {
                code.i32_const(0).local_set(register(destination));
            }
            code.local_get(FLAGS)
                .i32_const(!0x8d7)
                .i32_and()
                .i32_const(0x46)
                .i32_or()
                .local_set(FLAGS);
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
        _ => unreachable!("instruction routed to arithmetic emitter"),
    }
    store
}
