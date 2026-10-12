use super::{
    BlockType, ByteRegister, ByteValue, DoubleShiftKind, EffectiveAddress, FLAGS, InstructionSink,
    LHS, RESULT, RHS, Register32, ShiftCount, ShiftKind, SmallWidth, WordValue, byte_value,
    insert_byte, logical_flags, memory, register, word_value,
};

pub(super) fn shift_word(
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

pub(super) fn shift_memory_word(
    code: &mut InstructionSink<'_>,
    kind: ShiftKind,
    address: EffectiveAddress,
    count: ShiftCount,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
    code.local_set(LHS);
    shift_count(code, count);
    code.local_set(RHS);
    shift_word_value(code, kind);
    // checked store clobbers operand scratch; preserve the lossy inputs on the operand stack.
    code.local_get(LHS).local_get(RHS);
    memory::store_word_result(code, address, imports, exit_depth);
    code.local_set(RHS)
        .local_set(LHS)
        .local_get(RHS)
        .if_(BlockType::Empty);
    shift_word_flags(code, kind);
    code.end();
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

pub(super) fn shift_byte(
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

pub(super) fn shift_memory_byte(
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

pub(super) fn shift_memory_byte_immediate(
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

pub(super) fn shift_memory_byte_cl(
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

pub(super) fn shift(
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

pub(super) fn double_shift_word(
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
        .i32_const(!0x8d7)
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

pub(super) fn double_shift(
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

pub(super) fn double_shift_memory_word(
    code: &mut InstructionSink<'_>,
    kind: DoubleShiftKind,
    address: EffectiveAddress,
    source: Register32,
    count: ShiftCount,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
    code.local_tee(LHS).local_set(RESULT);
    shift_count(code, count);
    code.local_tee(RHS)
        .if_(BlockType::Empty)
        .local_get(RHS)
        .i32_const(16)
        .i32_gt_u()
        .if_(BlockType::Empty)
        .i32_const(0)
        .local_set(RESULT)
        .else_();
    word_value(code, WordValue::Register(source));
    code.local_set(RESULT);
    double_shift_word_value(code, kind);
    code.end().end();
    // store validation clobbers scratch; preserve the old word and count until flags commit.
    code.local_get(LHS).local_get(RHS);
    memory::store_word_result(code, address, imports, exit_depth);
    code.local_set(RHS)
        .local_set(LHS)
        .local_get(RHS)
        .if_(BlockType::Empty)
        .local_get(RHS)
        .i32_const(16)
        .i32_gt_u()
        .if_(BlockType::Empty)
        .local_get(FLAGS)
        .i32_const(!0x8d7)
        .i32_and()
        .i32_const(2)
        .i32_or()
        .local_set(FLAGS)
        .else_();
    double_shift_word_flags(code, kind);
    code.end().end();
}

pub(super) fn double_shift_memory(
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

pub(super) fn shift_memory(
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

pub(super) fn shift_count(code: &mut InstructionSink<'_>, count: ShiftCount) {
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
