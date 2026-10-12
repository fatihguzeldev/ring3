use super::{
    BlockType, ByteRegister, ByteValue, EffectiveAddress, FLAGS, InstructionSink, LHS, RESULT, RHS,
    Register32, RotateKind, ShiftCount, SmallWidth, WordValue, byte_value, insert_byte, memory,
    register, shift_count, word_value,
};

pub(super) fn rotate_word(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
    destination: Register32,
    count: ShiftCount,
) {
    word_value(code, WordValue::Register(destination));
    code.local_set(RESULT);
    shift_count(code, count);
    code.local_tee(RHS).if_(BlockType::Empty);
    rotate_word_value(code, kind);
    rotate_word_flags(code, kind);
    code.local_get(RESULT)
        .local_get(register(destination))
        .i32_const(!0xffff)
        .i32_and()
        .i32_or()
        .local_set(register(destination))
        .end();
}

pub(super) fn rotate_memory_word(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
    address: EffectiveAddress,
    count: ShiftCount,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
    code.local_set(RESULT);
    shift_count(code, count);
    code.local_set(RHS);
    rotate_word_value(code, kind);
    // flags use the original masked count, preserved across checked-store scratch.
    code.local_get(RHS);
    memory::store_word_result(code, address, imports, exit_depth);
    code.local_set(RHS).local_get(RHS).if_(BlockType::Empty);
    rotate_word_flags(code, kind);
    code.end();
}

fn rotate_word_value(code: &mut InstructionSink<'_>, kind: RotateKind) {
    code.local_get(RHS)
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
    code.i32_or().i32_const(0xffff).i32_and().local_set(RESULT);
}

fn rotate_word_flags(code: &mut InstructionSink<'_>, kind: RotateKind) {
    code.local_get(FLAGS)
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
        .end();
}

pub(super) fn byte_rotate_immediate(
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

pub(super) fn byte_rotate_cl(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
    destination: ByteRegister,
) {
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

pub(super) fn byte_rotate_one(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
    destination: ByteRegister,
) {
    byte_value(code, ByteValue::Register(destination));
    code.local_set(RESULT);
    byte_rotate_one_value(code, kind);
    code.local_get(RESULT);
    insert_byte(code, destination);
    byte_rotate_one_flags(code, kind);
}

pub(super) fn byte_rotate_one_value(code: &mut InstructionSink<'_>, kind: RotateKind) {
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

pub(super) fn byte_rotate_one_flags(code: &mut InstructionSink<'_>, kind: RotateKind) {
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

pub(super) fn byte_rotate_memory(
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

pub(super) fn rotate(
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

pub(super) fn rotate_memory(
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

pub(super) fn rotate_one(
    code: &mut InstructionSink<'_>,
    kind: RotateKind,
    destination: Register32,
) {
    code.local_get(register(destination)).local_set(RESULT);
    rotate_one_value(code, kind);
    code.local_get(RESULT).local_set(register(destination));
    rotate_one_flags(code, kind);
}

pub(super) fn rotate_one_value(code: &mut InstructionSink<'_>, kind: RotateKind) {
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

pub(super) fn rotate_one_flags(code: &mut InstructionSink<'_>, kind: RotateKind) {
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
