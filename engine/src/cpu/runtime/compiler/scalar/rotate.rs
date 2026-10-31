use super::*;

#[derive(Clone, Copy)]
enum Target {
    Byte(ByteRegister),
    Word(Register32),
    Dword(Register32),
    Memory(EffectiveAddress),
}

struct Rotate {
    kind: RotateKind,
    bits: u32,
    target: Target,
    count: ShiftCount,
}

fn lower(operation: Operation) -> Option<Rotate> {
    let (kind, bits, target, count) = match operation {
        Operation::ByteRotateImmediate {
            kind,
            destination,
            count,
        } => (
            kind,
            8,
            Target::Byte(destination),
            ShiftCount::Immediate(count),
        ),
        Operation::ByteRotateCl { kind, destination } => {
            (kind, 8, Target::Byte(destination), ShiftCount::Cl)
        }
        Operation::ByteRotateOne { kind, destination } => {
            (kind, 8, Target::Byte(destination), ShiftCount::Immediate(1))
        }
        Operation::MemoryByteRotateOne { kind, address } => {
            (kind, 8, Target::Memory(address), ShiftCount::Immediate(1))
        }
        Operation::MemoryByteRotate {
            kind,
            address,
            count,
        } => (kind, 8, Target::Memory(address), count),
        Operation::RotateOne { kind, destination } => (
            kind,
            32,
            Target::Dword(destination),
            ShiftCount::Immediate(1),
        ),
        Operation::Rotate {
            kind,
            destination,
            count,
        } => (kind, 32, Target::Dword(destination), count),
        Operation::MemoryRotate {
            kind,
            address,
            count,
        } => (kind, 32, Target::Memory(address), count),
        Operation::MemoryRotateOne { kind, address } => {
            (kind, 32, Target::Memory(address), ShiftCount::Immediate(1))
        }
        Operation::RotateWord {
            kind,
            destination,
            count,
        } => (kind, 16, Target::Word(destination), count),
        Operation::MemoryRotateWord {
            kind,
            address,
            count,
        } => (kind, 16, Target::Memory(address), count),
        _ => return None,
    };
    Some(Rotate {
        kind,
        bits,
        target,
        count,
    })
}

pub(super) fn supports(operation: &Operation) -> bool {
    lower(*operation).is_some()
        || matches!(
            operation,
            Operation::TranslateByte | Operation::TranslateByteAddress { .. }
        )
}

fn translate_byte(out: &mut Output, address_size: AddressSize, segment: SegmentRegister) {
    // The index is the old AL. Address-size wrapping precedes the segment base
    // and the byte load must succeed before any part of EAX is replaced.
    out.c()
        .local_get(reg(Register32::Ebx))
        .local_get(reg(Register32::Eax))
        .i32_const(255)
        .i32_and()
        .i32_add();
    if address_size == AddressSize::Bits16 {
        out.c().i32_const(0xffff).i32_and();
    }
    out.c().local_tee(OFFSET);
    match segment {
        SegmentRegister::Fs => {
            out.c().local_get(0).i32_load(mem(116)).i32_add();
        }
        SegmentRegister::Gs => {
            out.c().local_get(0).i32_load(mem(120)).i32_add();
        }
        _ => {}
    }
    out.c()
        .local_set(ADDRESS)
        .i32_const(segment.index() as i32)
        .local_set(SEGMENT);
    out.read(1, R);
    out.store_byte(ByteRegister::Al, R);
}

pub(super) fn emit(out: &mut Output, operation: Operation) -> bool {
    match operation {
        Operation::TranslateByte => {
            translate_byte(out, AddressSize::Bits32, SegmentRegister::Ds);
            return true;
        }
        Operation::TranslateByteAddress { address } => {
            translate_byte(out, address.address_size, address.resolved_segment());
            return true;
        }
        _ => {}
    }
    let Some(Rotate {
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
        Target::Dword(register) => {
            out.c().local_get(reg(register)).local_set(A);
        }
        Target::Memory(address) => {
            out.address(address);
            out.read(bits / 8 | 0x100, A);
        }
    }
    // B retains the masked count, including a nonzero full-byte/word cycle.
    // It is captured before a CL/CX/ECX destination can change its own count.
    out.count(count);
    if bits == 32 {
        out.c().local_get(A).local_get(B);
        match kind {
            RotateKind::Left => {
                out.c().i32_rotl();
            }
            RotateKind::Right => {
                out.c().i32_rotr();
            }
        }
    } else {
        out.c()
            .local_get(B)
            .i32_const((bits - 1) as i32)
            .i32_and()
            .local_set(TEMP)
            .local_get(A)
            .local_get(TEMP);
        match kind {
            RotateKind::Left => {
                out.c().i32_shl();
            }
            RotateKind::Right => {
                out.c().i32_shr_u();
            }
        }
        out.c()
            .local_get(A)
            .i32_const(bits as i32)
            .local_get(TEMP)
            .i32_sub();
        match kind {
            RotateKind::Left => {
                out.c().i32_shr_u();
            }
            RotateKind::Right => {
                out.c().i32_shl();
            }
        }
        out.c()
            .i32_or()
            .i32_const(((1_u32 << bits) - 1) as i32)
            .i32_and();
    }
    out.c().local_set(R);
    // Even a masked-zero count performs the checked Modify/read and write.
    // A failed write must leave both the old flags and the register state.
    match target {
        Target::Byte(register) => out.store_byte(register, R),
        Target::Word(register) => out.store_word(register, R),
        Target::Dword(register) => {
            out.c().local_get(R).local_set(reg(register));
        }
        Target::Memory(_) => out.write(bits / 8, R),
    }
    out.c().local_get(B).if_(BlockType::Empty).local_get(R);
    match kind {
        RotateKind::Left => {
            out.c().i32_const(1).i32_and();
        }
        RotateKind::Right => {
            out.c().i32_const((bits - 1) as i32).i32_shr_u();
        }
    }
    out.c()
        .local_set(CARRY)
        .local_get(FLAGS)
        .i32_wrap_i64()
        .i32_const(!0x801)
        .i32_and()
        .local_get(CARRY)
        .i32_or()
        .i64_extend_i32_u()
        .local_set(FLAGS)
        .local_get(B)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Empty)
        .local_get(FLAGS)
        .i32_wrap_i64()
        .local_get(R)
        .i32_const((bits - 1) as i32)
        .i32_shr_u();
    match kind {
        RotateKind::Left => {
            out.c().local_get(CARRY);
        }
        RotateKind::Right => {
            out.c()
                .local_get(R)
                .i32_const((bits - 2) as i32)
                .i32_shr_u();
        }
    }
    out.c()
        .i32_xor()
        .i32_const(1)
        .i32_and()
        .i32_const(11)
        .i32_shl()
        .i32_or()
        .i64_extend_i32_u()
        .local_set(FLAGS)
        .end()
        .end();
    true
}
