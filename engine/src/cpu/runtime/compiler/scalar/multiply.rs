use super::*;

const PRODUCT: u32 = 25;

pub(super) fn supports(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::SignedMultiply { .. }
            | Operation::SignedMultiplyWord { .. }
            | Operation::ReadSignedMultiplyWord { .. }
            | Operation::SignedMultiplyImmediateWord { .. }
            | Operation::ReadSignedMultiplyImmediateWord { .. }
            | Operation::MultiplyAccumulator { .. }
            | Operation::WordMultiplyAccumulator { .. }
            | Operation::ByteMultiplyAccumulator { .. }
            | Operation::ReadMultiplyAccumulator { .. }
            | Operation::ReadWordMultiplyAccumulator { .. }
            | Operation::ReadByteMultiplyAccumulator { .. }
            | Operation::DivideAccumulator { .. }
            | Operation::WordDivideAccumulator { .. }
            | Operation::ByteDivideAccumulator { .. }
            | Operation::ReadDivideAccumulator { .. }
            | Operation::ReadWordDivideAccumulator { .. }
            | Operation::ReadByteDivideAccumulator { .. }
    )
}

pub(super) fn emit(out: &mut Output, operation: Operation) -> bool {
    if emit_implicit(out, operation) {
        return true;
    }
    let (destination, bits) = match operation {
        Operation::SignedMultiply {
            destination,
            source,
            immediate,
        } => {
            out.location(source, A, false);
            match immediate {
                Some(value) => {
                    out.c().i32_const(value as i32).local_set(B);
                }
                None => {
                    out.c().local_get(reg(destination)).local_set(B);
                }
            }
            (destination, 32)
        }
        Operation::SignedMultiplyWord {
            destination,
            source,
        } => {
            out.word(WordValue::Register(destination), A);
            out.word(WordValue::Register(source), B);
            (destination, 16)
        }
        Operation::ReadSignedMultiplyWord {
            destination,
            address,
        } => {
            out.word(WordValue::Register(destination), A);
            out.address(address);
            out.read(2, B);
            (destination, 16)
        }
        Operation::SignedMultiplyImmediateWord {
            destination,
            source,
            immediate,
        } => {
            out.word(WordValue::Register(source), A);
            out.c().i32_const(i32::from(immediate)).local_set(B);
            (destination, 16)
        }
        Operation::ReadSignedMultiplyImmediateWord {
            destination,
            address,
            immediate,
        } => {
            out.address(address);
            out.read(2, A);
            out.c().i32_const(i32::from(immediate)).local_set(B);
            (destination, 16)
        }
        _ => return false,
    };
    for local in [A, B] {
        out.c().local_get(local);
        if bits == 16 {
            out.c().i32_const(16).i32_shl().i32_const(16).i32_shr_s();
        }
        out.c().i64_extend_i32_s();
    }
    out.c().i64_mul().local_tee(PRODUCT).i32_wrap_i64();
    if bits == 16 {
        out.c().i32_const(0xffff).i32_and();
    }
    out.c().local_set(R).local_get(PRODUCT).local_get(R);
    if bits == 16 {
        out.c().i32_const(16).i32_shl().i32_const(16).i32_shr_s();
    }
    out.c().i64_extend_i32_s().i64_ne().local_set(CARRY);
    if bits == 16 {
        out.store_word(destination, R);
    } else {
        out.c().local_get(R).local_set(reg(destination));
    }
    out.c()
        .local_get(FLAGS)
        .i32_const(!0x8d5)
        .i32_and()
        .local_get(CARRY)
        .i32_const(0x801)
        .i32_mul()
        .i32_or()
        .local_set(FLAGS);
    true
}

enum Source {
    Register(Register32),
    Byte(ByteRegister),
    Memory(EffectiveAddress),
}

fn emit_implicit(out: &mut Output, operation: Operation) -> bool {
    let (divide, signed, bits, source) = match operation {
        Operation::MultiplyAccumulator { kind, source } => (
            false,
            kind == MultiplyKind::Signed,
            32,
            Source::Register(source),
        ),
        Operation::WordMultiplyAccumulator { kind, source } => (
            false,
            kind == MultiplyKind::Signed,
            16,
            Source::Register(source),
        ),
        Operation::ByteMultiplyAccumulator { kind, source } => {
            (false, kind == MultiplyKind::Signed, 8, Source::Byte(source))
        }
        Operation::ReadMultiplyAccumulator { kind, address } => (
            false,
            kind == MultiplyKind::Signed,
            32,
            Source::Memory(address),
        ),
        Operation::ReadWordMultiplyAccumulator { kind, address } => (
            false,
            kind == MultiplyKind::Signed,
            16,
            Source::Memory(address),
        ),
        Operation::ReadByteMultiplyAccumulator { kind, address } => (
            false,
            kind == MultiplyKind::Signed,
            8,
            Source::Memory(address),
        ),
        Operation::DivideAccumulator { kind, source } => (
            true,
            kind == DivideKind::Signed,
            32,
            Source::Register(source),
        ),
        Operation::WordDivideAccumulator { kind, source } => (
            true,
            kind == DivideKind::Signed,
            16,
            Source::Register(source),
        ),
        Operation::ByteDivideAccumulator { kind, source } => {
            (true, kind == DivideKind::Signed, 8, Source::Byte(source))
        }
        Operation::ReadDivideAccumulator { kind, address } => (
            true,
            kind == DivideKind::Signed,
            32,
            Source::Memory(address),
        ),
        Operation::ReadWordDivideAccumulator { kind, address } => (
            true,
            kind == DivideKind::Signed,
            16,
            Source::Memory(address),
        ),
        Operation::ReadByteDivideAccumulator { kind, address } => {
            (true, kind == DivideKind::Signed, 8, Source::Memory(address))
        }
        _ => return false,
    };
    match source {
        Source::Register(register) => {
            out.c().local_get(reg(register));
            if bits == 16 {
                out.c().i32_const(0xffff).i32_and();
            }
            out.c().local_set(B);
        }
        Source::Byte(register) => out.byte(ByteValue::Register(register), B),
        Source::Memory(address) => {
            out.address(address);
            out.read(bits / 8, B);
        }
    }
    if divide {
        out.divide_implicit(bits, signed);
    } else {
        out.multiply_implicit(bits, signed);
    }
    true
}

impl Output {
    fn wide_operand(&mut self, local: u32, bits: u32, signed: bool) {
        self.c().local_get(local);
        if signed {
            if bits != 32 {
                self.c()
                    .i32_const((32 - bits) as i32)
                    .i32_shl()
                    .i32_const((32 - bits) as i32)
                    .i32_shr_s();
            }
            self.c().i64_extend_i32_s();
        } else {
            self.c().i64_extend_i32_u();
        }
    }

    fn multiply_implicit(&mut self, bits: u32, signed: bool) {
        self.c().local_get(reg(Register32::Eax));
        if bits != 32 {
            self.c().i32_const(((1_u32 << bits) - 1) as i32).i32_and();
        }
        self.c().local_set(A);
        self.wide_operand(A, bits, signed);
        self.wide_operand(B, bits, signed);
        self.c().i64_mul().local_tee(PRODUCT).i32_wrap_i64();
        if bits == 8 {
            self.c().i32_const(0xffff).i32_and();
        } else if bits == 16 {
            self.c().i32_const(0xffff).i32_and();
        }
        self.c().local_set(R).local_get(PRODUCT);
        if signed {
            self.wide_operand(R, bits, true);
            self.c().i64_ne();
        } else {
            self.c().i64_const(((1_u64 << bits) - 1) as i64).i64_gt_u();
        }
        self.c().local_set(CARRY);
        if bits == 8 {
            self.store_word(Register32::Eax, R);
        } else {
            self.c()
                .local_get(PRODUCT)
                .i64_const(bits as i64)
                .i64_shr_u()
                .i32_wrap_i64()
                .local_set(A);
            if bits == 16 {
                self.store_word(Register32::Eax, R);
                self.store_word(Register32::Edx, A);
            } else {
                self.c()
                    .local_get(R)
                    .local_set(reg(Register32::Eax))
                    .local_get(A)
                    .local_set(reg(Register32::Edx));
            }
        }
        self.c()
            .local_get(FLAGS)
            .i32_const(!0x8d5)
            .i32_and()
            .local_get(CARRY)
            .i32_const(0x801)
            .i32_mul()
            .i32_or()
            .local_set(FLAGS);
    }

    fn dividend(&mut self, bits: u32, signed: bool) {
        match bits {
            8 => {
                self.c()
                    .local_get(reg(Register32::Eax))
                    .i32_const(0xffff)
                    .i32_and();
                if signed {
                    self.c()
                        .i32_const(16)
                        .i32_shl()
                        .i32_const(16)
                        .i32_shr_s()
                        .i64_extend_i32_s();
                } else {
                    self.c().i64_extend_i32_u();
                }
            }
            16 => {
                self.c()
                    .local_get(reg(Register32::Edx))
                    .i32_const(16)
                    .i32_shl()
                    .local_get(reg(Register32::Eax))
                    .i32_const(0xffff)
                    .i32_and()
                    .i32_or();
                if signed {
                    self.c().i64_extend_i32_s();
                } else {
                    self.c().i64_extend_i32_u();
                }
            }
            _ => {
                self.c()
                    .local_get(reg(Register32::Edx))
                    .i64_extend_i32_u()
                    .i64_const(32)
                    .i64_shl()
                    .local_get(reg(Register32::Eax))
                    .i64_extend_i32_u()
                    .i64_or();
            }
        }
    }

    fn divide_fault(&mut self) {
        self.c()
            .local_get(0)
            .i32_const(0)
            .i32_store(mem(60))
            .local_get(0)
            .i32_const(0)
            .i32_store(mem(64))
            .local_get(0)
            .i32_const(0)
            .i32_store(mem(68));
        self.exit(9);
    }

    fn divide_implicit(&mut self, bits: u32, signed: bool) {
        self.c().local_get(B).i32_eqz().if_(BlockType::Empty);
        self.divide_fault();
        self.c().end();
        self.dividend(bits, signed);
        self.c().local_set(PRODUCT);
        if signed && bits == 32 {
            self.c()
                .local_get(PRODUCT)
                .i64_const(i64::MIN)
                .i64_eq()
                .local_get(B)
                .i32_const(-1)
                .i32_eq()
                .i32_and()
                .if_(BlockType::Empty);
            self.divide_fault();
            self.c().end();
        }
        self.c().local_get(PRODUCT);
        self.wide_operand(B, bits, signed);
        if signed {
            self.c().i64_div_s();
        } else {
            self.c().i64_div_u();
        }
        self.c().local_set(PRODUCT);
        if signed {
            let limit = 1_i64 << (bits - 1);
            self.c()
                .local_get(PRODUCT)
                .i64_const(-limit)
                .i64_lt_s()
                .local_get(PRODUCT)
                .i64_const(limit)
                .i64_ge_s()
                .i32_or();
        } else {
            self.c()
                .local_get(PRODUCT)
                .i64_const(((1_u64 << bits) - 1) as i64)
                .i64_gt_u();
        }
        self.c().if_(BlockType::Empty);
        self.divide_fault();
        self.c()
            .end()
            .local_get(PRODUCT)
            .i32_wrap_i64()
            .local_set(R);
        self.dividend(bits, signed);
        self.wide_operand(B, bits, signed);
        if signed {
            self.c().i64_rem_s();
        } else {
            self.c().i64_rem_u();
        }
        self.c().i32_wrap_i64().local_set(A);
        if bits == 8 {
            self.c()
                .local_get(R)
                .i32_const(255)
                .i32_and()
                .local_get(A)
                .i32_const(255)
                .i32_and()
                .i32_const(8)
                .i32_shl()
                .i32_or()
                .local_set(R);
            self.store_word(Register32::Eax, R);
        } else if bits == 16 {
            self.store_word(Register32::Eax, R);
            self.store_word(Register32::Edx, A);
        } else {
            self.c()
                .local_get(R)
                .local_set(reg(Register32::Eax))
                .local_get(A)
                .local_set(reg(Register32::Edx));
        }
    }
}
