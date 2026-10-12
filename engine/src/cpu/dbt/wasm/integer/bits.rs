use super::{
    ADDRESS, BitIndex, BitScanKind, BitTestKind, BlockType, EffectiveAddress, ExtensionKind, FLAGS,
    InstructionSink, LHS, RESULT, RHS, Register32, SmallWidth, extend_value, memory, register,
};

pub(super) fn trailing_zero_count32(
    code: &mut InstructionSink<'_>,
    destination: Register32,
    source: u32,
) {
    code.local_get(source)
        .local_set(LHS)
        .local_get(LHS)
        .i32_ctz()
        .local_set(RESULT)
        .local_get(FLAGS)
        .i32_const(!0x8d5)
        .i32_and()
        .local_get(LHS)
        .i32_eqz()
        .i32_or()
        .local_get(RESULT)
        .i32_eqz()
        .i32_const(6)
        .i32_shl()
        .i32_or()
        .local_set(FLAGS)
        .local_get(RESULT)
        .local_set(register(destination));
}

pub(super) fn bit_scan_word(
    code: &mut InstructionSink<'_>,
    kind: BitScanKind,
    destination: Register32,
    source: u32,
) {
    code.local_get(source)
        .i32_const(0xffff)
        .i32_and()
        .local_set(RESULT)
        .local_get(register(destination))
        .i32_const(!0xffff)
        .i32_and();
    bit_scan(code, kind, destination, RESULT);
    code.local_get(register(destination))
        .i32_const(0xffff)
        .i32_and()
        .i32_or()
        .local_set(register(destination));
}

pub(super) fn bit_scan(
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
        .i32_const(!0x8d5)
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

pub(super) fn memory_bit_test_word(
    code: &mut InstructionSink<'_>,
    kind: BitTestKind,
    address: EffectiveAddress,
    index: BitIndex,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::address_value(code, address);
    if let BitIndex::Register(source) = index {
        code.local_get(register(source));
        extend_value(code, ExtensionKind::Sign, SmallWidth::Word);
        code.i32_const(4)
            .i32_shr_s()
            .i32_const(1)
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
    code.i32_const(15).i32_and();
    memory::load_narrow_at_address(code, SmallWidth::Word, imports, exit_depth);
    code.local_set(RESULT)
        .local_set(RHS)
        .local_get(FLAGS)
        .i32_const(!0x895)
        .i32_and()
        .local_get(RESULT)
        .local_get(RHS)
        .i32_shr_u()
        .i32_const(1)
        .i32_and()
        .i32_or();
    if kind != BitTestKind::Test {
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
            BitTestKind::Test => unreachable!(),
        }
        code.local_set(RESULT);
        memory::store_word_result_at_address(code, imports, exit_depth);
    }
    code.local_set(FLAGS);
}

pub(super) fn memory_bit_mutation(
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
        .i32_const(!0x895)
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

pub(super) fn bit_test(
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
        .i32_const(!0x895)
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

pub(super) fn bit_test_word(
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
    code.i32_const(15)
        .i32_and()
        .local_set(RHS)
        .local_get(FLAGS)
        .i32_const(!0x895)
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
