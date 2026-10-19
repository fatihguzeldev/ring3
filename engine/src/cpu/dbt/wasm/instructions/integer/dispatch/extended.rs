use super::*;

pub(super) fn emit(
    code: &mut InstructionSink<'_>,
    instruction: &DecodedOperation,
    imports: Imports,
    exit_depth: u32,
) -> bool {
    let mut store = false;
    match *instruction.operation() {
        Operation::QuerySegment {
            kind,
            source,
            destination,
            word,
        } => {
            crate::cpu::dbt::wasm::instructions::segment_queries::query(
                code,
                kind,
                source,
                destination,
                word,
                imports,
                exit_depth,
            );
        }
        Operation::VerifySegment { source, write } => {
            crate::cpu::dbt::wasm::instructions::segment_queries::verify(
                code, source, write, imports, exit_depth,
            );
        }
        Operation::XState(_) => {
            store = crate::cpu::dbt::wasm::instructions::xstate::instruction(
                code,
                instruction,
                imports,
                exit_depth,
            );
        }
        Operation::AdjustPrivilege {
            destination,
            source,
        } => {
            store = crate::cpu::dbt::wasm::instructions::segment_queries::adjust_privilege(
                code,
                destination,
                source,
                imports,
                exit_depth,
            );
        }
        Operation::CacheFlush { address } => {
            crate::cpu::dbt::wasm::instructions::cache_flush::instruction(
                code, address, imports, exit_depth,
            );
        }
        Operation::VirtualTimestampAux => {
            crate::cpu::dbt::wasm::instructions::integer_system::timestamp(
                code, true, imports, exit_depth,
            );
        }
        Operation::PackedStringCompare(operation) => {
            store = crate::cpu::dbt::wasm::instructions::packed_strings::instruction(
                code, operation, imports, exit_depth,
            );
        }
        Operation::Crc32 {
            destination,
            source,
        } => {
            super::super::scalar_bits::crc32(code, destination, source, imports, exit_depth);
        }
        Operation::MultiplyExtended { high, low, source } => {
            super::super::bit_manipulation::multiply_extended(
                code, high, low, source, imports, exit_depth,
            );
        }
        Operation::BitManipulation {
            kind,
            destination,
            source,
        } => {
            super::super::bit_manipulation::bit_manipulation(
                code,
                kind,
                destination,
                source,
                imports,
                exit_depth,
            );
        }
        Operation::BitCount {
            kind,
            width,
            destination,
            source,
        } => {
            super::super::scalar_bits::bit_count(
                code,
                kind,
                width,
                destination,
                source,
                imports,
                exit_depth,
            );
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
                crate::cpu::dbt::wasm::helpers::result::ResultContext::from(imports),
                imports.native_success,
            );
            store = operation.write_size() != 0;
        }
        Operation::Avx(operation) => {
            store = crate::cpu::dbt::wasm::instructions::avx::instruction(
                code, operation, imports, exit_depth,
            )
        }
        Operation::Simd(operation) => {
            store = simd::instruction(code, operation, imports, exit_depth)
        }
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
        Operation::SetAlFromCarry => {
            conditions::condition(code, Condition::Below);
            code.i32_const(0xff).i32_mul();
            insert_byte(code, ByteRegister::Al);
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
        Operation::CpuId => integer_system::cpuid(code),
        Operation::ReadProcessorId { destination } => {
            integer_system::processor_id(code, destination);
        }
        Operation::VirtualTimestamp => integer_system::timestamp(code, false, imports, exit_depth),
        _ => unreachable!("instruction routed to extended emitter"),
    }
    store
}
