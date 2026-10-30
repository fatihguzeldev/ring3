use super::*;

#[derive(Clone, Copy)]
enum Target {
    Byte(ByteRegister),
    Word(Register32),
    Memory(EffectiveAddress),
}

struct ShiftOperation {
    kind: ShiftKind,
    bits: u32,
    target: Target,
    count: ShiftCount,
}

fn lower(operation: Operation) -> Option<ShiftOperation> {
    let (kind, bits, target, count) = match operation {
        Operation::ShiftByte {
            kind,
            destination,
            count,
        } => (kind, 8, Target::Byte(destination), count),
        Operation::ShiftWord {
            kind,
            destination,
            count,
        } => (kind, 16, Target::Word(destination), count),
        Operation::MemoryShiftByte { kind, address } => {
            (kind, 8, Target::Memory(address), ShiftCount::Immediate(1))
        }
        Operation::MemoryShiftByteImmediate {
            kind,
            address,
            count,
        } => (
            kind,
            8,
            Target::Memory(address),
            ShiftCount::Immediate(count),
        ),
        Operation::MemoryShiftByteCl { kind, address } => {
            (kind, 8, Target::Memory(address), ShiftCount::Cl)
        }
        Operation::MemoryShiftWord {
            kind,
            address,
            count,
        } => (kind, 16, Target::Memory(address), count),
        _ => return None,
    };
    Some(ShiftOperation {
        kind,
        bits,
        target,
        count,
    })
}

pub(super) fn supports(operation: &Operation) -> bool {
    lower(*operation).is_some()
}

pub(super) fn emit(out: &mut Output, operation: Operation) -> bool {
    let Some(ShiftOperation {
        kind,
        bits,
        target,
        count,
    }) = lower(operation)
    else {
        return false;
    };
    match target {
        Target::Byte(register) => out.byte(ByteValue::Register(register), A),
        Target::Word(register) => out.word(WordValue::Register(register), A),
        Target::Memory(address) => {
            out.address(address);
            out.read(bits / 8 | 0x100, A);
        }
    }
    out.count(count);
    out.c().local_get(A);
    if kind == ShiftKind::Sar {
        out.c()
            .i32_const((32 - bits) as i32)
            .i32_shl()
            .i32_const((32 - bits) as i32)
            .i32_shr_s();
    }
    out.c().local_get(B);
    match kind {
        ShiftKind::Shl => {
            out.c().i32_shl();
        }
        ShiftKind::Shr => {
            out.c().i32_shr_u();
        }
        ShiftKind::Sar => {
            out.c().i32_shr_s();
        }
    }
    out.c()
        .i32_const(((1_u32 << bits) - 1) as i32)
        .i32_and()
        .local_set(R);
    match target {
        Target::Byte(register) => out.store_byte(register, R),
        Target::Word(register) => out.store_word(register, R),
        Target::Memory(_) => out.write(bits / 8, R),
    }
    out.c().local_get(B).if_(BlockType::Empty);
    out.narrow_shift_flags(kind, bits);
    out.c().end();
    true
}

impl Output {
    fn narrow_shift_flags(&mut self, kind: ShiftKind, bits: u32) {
        self.c()
            .local_get(B)
            .i32_const(bits as i32)
            .i32_lt_u()
            .if_(BlockType::Result(ValType::I32))
            .local_get(A);
        if kind == ShiftKind::Shl {
            self.c().i32_const(bits as i32).local_get(B).i32_sub();
        } else {
            self.c().local_get(B).i32_const(1).i32_sub();
        }
        self.c().i32_shr_u().i32_const(1).i32_and().else_();
        if kind == ShiftKind::Sar {
            self.c()
                .local_get(A)
                .i32_const((bits - 1) as i32)
                .i32_shr_u();
        } else {
            self.c().i32_const(0);
        }
        self.c()
            .end()
            .local_set(CARRY)
            .local_get(FLAGS)
            .i32_const(!0x8d5)
            .i32_and()
            .local_get(CARRY)
            .i32_or()
            .local_get(R)
            .i32_eqz()
            .i32_const(6)
            .i32_shl()
            .i32_or()
            .local_get(R)
            .i32_const((bits - 1) as i32)
            .i32_shr_u()
            .i32_const(7)
            .i32_shl()
            .i32_or()
            .local_get(R)
            .i32_const(255)
            .i32_and()
            .i32_popcnt()
            .i32_const(1)
            .i32_and()
            .i32_eqz()
            .i32_const(2)
            .i32_shl()
            .i32_or()
            .local_set(FLAGS)
            .local_get(B)
            .i32_const(1)
            .i32_eq()
            .if_(BlockType::Empty)
            .local_get(FLAGS);
        match kind {
            ShiftKind::Shl => {
                self.c()
                    .local_get(R)
                    .i32_const((bits - 1) as i32)
                    .i32_shr_u()
                    .local_get(CARRY)
                    .i32_xor();
            }
            ShiftKind::Shr => {
                self.c()
                    .local_get(A)
                    .i32_const((bits - 1) as i32)
                    .i32_shr_u();
            }
            ShiftKind::Sar => {
                self.c().i32_const(0);
            }
        }
        self.c()
            .i32_const(11)
            .i32_shl()
            .i32_or()
            .local_set(FLAGS)
            .end();
    }
}
