use super::{
    BlockType, ByteRegister, ByteValue, EffectiveAddress, FLAGS, InstructionSink, LHS, RESULT, RHS,
    Register32, RotateKind, ShiftCount, SmallWidth, ValType, WordValue, byte_value, insert_byte,
    memory, register, shift_count, word_value,
};

pub(super) fn rotate_through_carry_word(
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
        .if_(BlockType::Empty);
    rotate_through_carry_word_value(code, kind);
    rotate_through_carry_word_flags(code, kind);
    code.local_get(RESULT)
        .i32_const(0xffff)
        .i32_and()
        .local_get(register(destination))
        .i32_const(!0xffff)
        .i32_and()
        .i32_or()
        .local_set(register(destination))
        .end();
}

pub(super) fn rotate_through_carry_memory_word(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
    address: EffectiveAddress,
    count: ShiftCount,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::load_narrow_for_modify(code, address, SmallWidth::Word, imports, exit_depth);
    code.local_set(RESULT);
    shift_count(code, count);
    code.local_tee(RHS).i32_const(17).i32_rem_u().local_set(LHS);
    rotate_through_carry_word_value(code, kind);
    // preserve the full carry ring and original count while the helper stores only low16.
    code.local_get(RESULT)
        .local_get(RHS)
        .local_get(RESULT)
        .i32_const(0xffff)
        .i32_and()
        .local_set(RESULT);
    memory::store_word_result(code, address, imports, exit_depth);
    code.local_set(RHS)
        .local_set(RESULT)
        .local_get(RHS)
        .i32_const(17)
        .i32_rem_u()
        .if_(BlockType::Empty);
    rotate_through_carry_word_flags(code, kind);
    code.end();
}

fn rotate_through_carry_word_value(code: &mut InstructionSink<'_>, kind: RotateKind) {
    code.local_get(RESULT)
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
    code.i32_or().i32_const(0x1ffff).i32_and().local_set(RESULT);
}

fn rotate_through_carry_word_flags(code: &mut InstructionSink<'_>, kind: RotateKind) {
    code.local_get(FLAGS)
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
        .end();
}

pub(super) fn byte_rotate_through_carry_one(
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

pub(super) fn byte_rotate_through_carry_one_value(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
) {
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

pub(super) fn byte_rotate_through_carry_one_flags(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
) {
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

pub(super) fn byte_rotate_through_carry_cl(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
    destination: ByteRegister,
) {
    byte_rotate_through_carry(code, kind, destination, ShiftCount::Cl);
}

pub(super) fn byte_rotate_through_carry_memory_immediate(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
    address: EffectiveAddress,
    count: u8,
    imports: memory::Imports,
    exit_depth: u32,
) {
    // masked-one instructions use the existing One path with its defined OF.
    let distance = i32::from((count & 31) % 9);
    memory::load_narrow_for_modify(code, address, SmallWidth::Byte, imports, exit_depth);
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

pub(super) fn byte_rotate_through_carry_memory_cl(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
    address: EffectiveAddress,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::load_narrow_for_modify(code, address, SmallWidth::Byte, imports, exit_depth);
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

pub(super) fn byte_rotate_through_carry(
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

pub(super) fn rotate_through_carry_immediate(
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

pub(super) fn memory_rotate_through_carry_immediate(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
    address: EffectiveAddress,
    raw: u8,
    imports: memory::Imports,
    exit_depth: u32,
) {
    let count = i32::from(raw & 31);
    memory::load_modify_result(code, address, imports, exit_depth);
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

pub(super) fn memory_rotate_through_carry_cl(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
    address: EffectiveAddress,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::load_modify_result(code, address, imports, exit_depth);
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

pub(super) fn rotate_through_carry_cl(
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

pub(super) fn rotate_through_carry_one(
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

pub(super) fn rotate_through_carry_one_value(code: &mut InstructionSink<'_>, kind: RotateKind) {
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

pub(super) fn rotate_through_carry_one_flags(code: &mut InstructionSink<'_>, kind: RotateKind) {
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
