use super::*;

pub(super) fn emit(
    code: &mut InstructionSink<'_>,
    instruction: &DecodedOperation,
    imports: Imports,
    exit_depth: u32,
) -> bool {
    let mut store = false;
    match *instruction.operation() {
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
        _ => unreachable!("instruction routed to shifts emitter"),
    }
    store
}
