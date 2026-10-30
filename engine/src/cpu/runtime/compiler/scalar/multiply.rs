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
    )
}

pub(super) fn emit(out: &mut Output, operation: Operation) -> bool {
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
