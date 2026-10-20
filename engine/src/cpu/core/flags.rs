use wasm_encoder::{BlockType, Function, InstructionSink, ValType};

pub(crate) const CF: u32 = 1;
pub(crate) const PF: u32 = 1 << 2;
pub(crate) const AF: u32 = 1 << 4;
pub(crate) const ZF: u32 = 1 << 6;
pub(crate) const SF: u32 = 1 << 7;
pub(crate) const OF: u32 = 1 << 11;
pub(crate) const ARITHMETIC: u32 = CF | PF | AF | ZF | SF | OF;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum FlagKind {
    None = 0,
    Add = 1,
    Sub = 2,
    Logical = 3,
    Inc = 4,
    Dec = 5,
    Adc = 6,
    Sbb = 7,
    Neg = 8,
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub(crate) fn materialize(
    base: u32,
    kind: FlagKind,
    width: u32,
    lhs: u32,
    rhs: u32,
    result: u32,
    carry: u32,
    mask: u32,
) -> u32 {
    let base = base | 2;
    if kind == FlagKind::None || !matches!(width, 8 | 16 | 32) {
        return base;
    }
    let written = mask
        & ARITHMETIC
        & if matches!(kind, FlagKind::Inc | FlagKind::Dec) {
            !CF
        } else {
            u32::MAX
        };
    let width_mask = u32::MAX >> (32 - width);
    let sign = 1 << (width - 1);
    let mut left = lhs & width_mask;
    let mut right = rhs & width_mask;
    let value = result & width_mask;
    match kind {
        FlagKind::Inc | FlagKind::Dec => right = 1,
        FlagKind::Neg => {
            right = left;
            left = 0;
        }
        _ => {}
    }
    let mut bits = 0;
    if written & CF != 0 && kind != FlagKind::Logical {
        let carry = u64::from(carry & 1);
        let flag = if matches!(kind, FlagKind::Add | FlagKind::Adc) {
            u64::from(left) + u64::from(right) + if kind == FlagKind::Adc { carry } else { 0 }
                > u64::from(width_mask)
        } else {
            u64::from(left) < u64::from(right) + if kind == FlagKind::Sbb { carry } else { 0 }
        };
        bits |= u32::from(flag);
    }
    if written & PF != 0 {
        bits |= u32::from((value as u8).count_ones() % 2 == 0) << 2;
    }
    if written & AF != 0 && kind != FlagKind::Logical {
        bits |= (left ^ right ^ value) & AF;
    }
    if written & ZF != 0 {
        bits |= u32::from(value == 0) << 6;
    }
    if written & SF != 0 {
        bits |= u32::from(value & sign != 0) << 7;
    }
    if written & OF != 0 && kind != FlagKind::Logical {
        let relation = if matches!(kind, FlagKind::Add | FlagKind::Adc | FlagKind::Inc) {
            !(left ^ right)
        } else {
            left ^ right
        };
        bits |= u32::from(relation & (left ^ value) & sign != 0) << 11;
    }
    ((base & !written) | (bits & written)) | 2
}

const LEFT: u32 = 8;
const RIGHT: u32 = 9;
const VALUE: u32 = 10;
const SIGN: u32 = 11;
const REQUESTED: u32 = 12;
const BITS: u32 = 13;
const WIDTH_MASK: u32 = 14;

// params are base, kind, width, lhs, rhs, result, carry, requested mask.
pub(crate) fn emit_function() -> Function {
    let mut function = Function::new([(7, ValType::I32)]);
    let mut code = function.instructions();
    code.local_get(0).i32_const(2).i32_or().local_set(0);
    code.local_get(1)
        .i32_const(1)
        .i32_lt_u()
        .local_get(1)
        .i32_const(8)
        .i32_gt_u()
        .i32_or();
    return_base_if(&mut code);
    code.local_get(2)
        .i32_const(8)
        .i32_ne()
        .local_get(2)
        .i32_const(16)
        .i32_ne()
        .i32_and()
        .local_get(2)
        .i32_const(32)
        .i32_ne()
        .i32_and();
    return_base_if(&mut code);
    code.local_get(7)
        .i32_const(ARITHMETIC as i32)
        .i32_and()
        .local_set(REQUESTED);
    kind_is(&mut code, FlagKind::Inc);
    kind_is(&mut code, FlagKind::Dec);
    code.i32_or()
        .if_(BlockType::Empty)
        .local_get(REQUESTED)
        .i32_const(!CF as i32)
        .i32_and()
        .local_set(REQUESTED)
        .end();
    code.i32_const(-1)
        .i32_const(32)
        .local_get(2)
        .i32_sub()
        .i32_shr_u()
        .local_set(WIDTH_MASK)
        .i32_const(1)
        .local_get(2)
        .i32_const(1)
        .i32_sub()
        .i32_shl()
        .local_set(SIGN);
    for (parameter, local) in [(3, LEFT), (4, RIGHT), (5, VALUE)] {
        code.local_get(parameter)
            .local_get(WIDTH_MASK)
            .i32_and()
            .local_set(local);
    }
    kind_is(&mut code, FlagKind::Inc);
    kind_is(&mut code, FlagKind::Dec);
    code.i32_or()
        .if_(BlockType::Empty)
        .i32_const(1)
        .local_set(RIGHT)
        .end();
    kind_is(&mut code, FlagKind::Neg);
    code.if_(BlockType::Empty)
        .local_get(LEFT)
        .local_set(RIGHT)
        .i32_const(0)
        .local_set(LEFT)
        .end();
    code.i32_const(0).local_set(BITS);

    requested(&mut code, CF);
    not_logical(&mut code);
    kind_is(&mut code, FlagKind::Add);
    kind_is(&mut code, FlagKind::Adc);
    code.i32_or()
        .if_(BlockType::Result(ValType::I32))
        .local_get(LEFT)
        .i64_extend_i32_u()
        .local_get(RIGHT)
        .i64_extend_i32_u()
        .i64_add();
    carry_input(&mut code, FlagKind::Adc);
    code.i64_add()
        .local_get(WIDTH_MASK)
        .i64_extend_i32_u()
        .i64_gt_u()
        .else_()
        .local_get(LEFT)
        .i64_extend_i32_u()
        .local_get(RIGHT)
        .i64_extend_i32_u();
    carry_input(&mut code, FlagKind::Sbb);
    code.i64_add().i64_lt_u().end();
    merge(&mut code);
    code.end().end();

    requested(&mut code, PF);
    code.local_get(VALUE)
        .i32_const(255)
        .i32_and()
        .i32_popcnt()
        .i32_const(1)
        .i32_and()
        .i32_eqz()
        .i32_const(2)
        .i32_shl();
    merge(&mut code);
    code.end();
    requested(&mut code, AF);
    not_logical(&mut code);
    code.local_get(LEFT)
        .local_get(RIGHT)
        .i32_xor()
        .local_get(VALUE)
        .i32_xor()
        .i32_const(AF as i32)
        .i32_and();
    merge(&mut code);
    code.end().end();
    requested(&mut code, ZF);
    code.local_get(VALUE).i32_eqz().i32_const(6).i32_shl();
    merge(&mut code);
    code.end();
    requested(&mut code, SF);
    code.local_get(VALUE)
        .local_get(SIGN)
        .i32_and()
        .i32_const(0)
        .i32_ne()
        .i32_const(7)
        .i32_shl();
    merge(&mut code);
    code.end();
    requested(&mut code, OF);
    not_logical(&mut code);
    kind_is(&mut code, FlagKind::Add);
    kind_is(&mut code, FlagKind::Adc);
    code.i32_or();
    kind_is(&mut code, FlagKind::Inc);
    code.i32_or()
        .if_(BlockType::Result(ValType::I32))
        .local_get(LEFT)
        .local_get(RIGHT)
        .i32_xor()
        .i32_const(-1)
        .i32_xor()
        .else_()
        .local_get(LEFT)
        .local_get(RIGHT)
        .i32_xor()
        .end()
        .local_get(LEFT)
        .local_get(VALUE)
        .i32_xor()
        .i32_and()
        .local_get(SIGN)
        .i32_and()
        .i32_const(0)
        .i32_ne()
        .i32_const(11)
        .i32_shl();
    merge(&mut code);
    code.end().end();
    code.local_get(0)
        .local_get(REQUESTED)
        .i32_const(-1)
        .i32_xor()
        .i32_and()
        .local_get(BITS)
        .local_get(REQUESTED)
        .i32_and()
        .i32_or()
        .i32_const(2)
        .i32_or()
        .end();
    function
}

fn return_base_if(code: &mut InstructionSink<'_>) {
    code.if_(BlockType::Empty).local_get(0).return_().end();
}

fn kind_is(code: &mut InstructionSink<'_>, kind: FlagKind) {
    code.local_get(1).i32_const(kind as i32).i32_eq();
}

fn requested(code: &mut InstructionSink<'_>, flag: u32) {
    code.local_get(REQUESTED)
        .i32_const(flag as i32)
        .i32_and()
        .if_(BlockType::Empty);
}

fn not_logical(code: &mut InstructionSink<'_>) {
    code.local_get(1)
        .i32_const(FlagKind::Logical as i32)
        .i32_ne()
        .if_(BlockType::Empty);
}

fn carry_input(code: &mut InstructionSink<'_>, kind: FlagKind) {
    kind_is(code, kind);
    code.local_get(6)
        .i32_const(1)
        .i32_and()
        .i32_and()
        .i64_extend_i32_u();
}

fn merge(code: &mut InstructionSink<'_>) {
    code.local_get(BITS).i32_or().local_set(BITS);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_arithmetic_boundaries_cover_carry_overflow_and_partial_writes() {
        let cases = [
            (FlagKind::Add, 8, 0xff, 1, 0, 0, 0, 0x57),
            (FlagKind::Add, 8, 0x7f, 1, 0x80, 0, 0, 0x892),
            (FlagKind::Sub, 8, 0, 1, 0xff, 0, 0, 0x97),
            (FlagKind::Sub, 8, 0x80, 1, 0x7f, 0, 0, 0x812),
            (FlagKind::Logical, 32, 0, 0, 0, 0, ARITHMETIC, 0x46),
            (FlagKind::Inc, 8, 0xff, 0, 0, 0, CF, 0x57),
            (FlagKind::Dec, 8, 0x80, 0, 0x7f, 0, CF, 0x813),
            (FlagKind::Adc, 8, 0xff, 0, 0, 1, 0, 0x57),
            (FlagKind::Sbb, 8, 0, 0, 0xff, 1, 0, 0x97),
            (FlagKind::Neg, 8, 1, 0, 0xff, 0, 0, 0x97),
            (FlagKind::Neg, 8, 0x80, 0, 0x80, 0, 0, 0x883),
            (FlagKind::Neg, 32, 0, 0, 0, 0, 0, 0x46),
            (FlagKind::Add, 16, 0xffff, 1, 0, 0, 0, 0x57),
            (FlagKind::Add, 32, u32::MAX, 1, 0, 0, 0, 0x57),
            (FlagKind::Sub, 32, 0x8000_0000, 1, 0x7fff_ffff, 0, 0, 0x816),
        ];
        for (kind, width, lhs, rhs, result, carry, base, expected) in cases {
            assert_eq!(
                materialize(base, kind, width, lhs, rhs, result, carry, ARITHMETIC),
                expected,
                "{kind:?}/{width} {lhs:x} {rhs:x} {result:x}"
            );
        }
    }

    #[test]
    fn requested_bits_are_the_only_observable_change() {
        let base = 0xa5a5_f7f5;
        assert_eq!(
            materialize(base, FlagKind::None, 32, 0, 0, 0, 0, u32::MAX),
            base | 2
        );
        assert_eq!(
            materialize(base, FlagKind::Add, 32, 1, 1, 2, 0, 0),
            base | 2
        );
        assert_eq!(
            materialize(base, FlagKind::Add, 7, 1, 1, 2, 0, ARITHMETIC),
            base | 2
        );
        assert_eq!(
            materialize(base, FlagKind::Inc, 32, 0, 1, 1, 0, CF),
            base | 2
        );
        for mask in 0..=ARITHMETIC {
            let actual = materialize(base, FlagKind::Sub, 8, 0, 1, 0xff, 0, mask);
            let writable = mask & ARITHMETIC;
            assert_eq!(actual, ((base & !writable) | (0x95 & writable)) | 2);
        }
    }

    #[test]
    fn exhaustive_byte_add_sub_carry_and_borrow_match_widened_integer_oracles() {
        for lhs in 0..=255_u32 {
            for rhs in 0..=255_u32 {
                for (kind, subtract, carry) in [
                    (FlagKind::Add, false, 0),
                    (FlagKind::Sub, true, 0),
                    (FlagKind::Adc, false, 1),
                    (FlagKind::Sbb, true, 1),
                ] {
                    let signed_lhs = i16::from(lhs as u8 as i8);
                    let signed_rhs = i16::from(rhs as u8 as i8);
                    let signed = if subtract {
                        signed_lhs - signed_rhs - carry as i16
                    } else {
                        signed_lhs + signed_rhs + carry as i16
                    };
                    let wide = if subtract {
                        i32::from(lhs as u8) - i32::from(rhs as u8) - carry as i32
                    } else {
                        i32::from(lhs as u8) + i32::from(rhs as u8) + carry as i32
                    };
                    let result = (wide & 255) as u32;
                    let cf = if subtract { wide < 0 } else { wide > 255 };
                    let nibble = if subtract {
                        (lhs & 15) as i32 - (rhs & 15) as i32 - carry as i32
                    } else {
                        (lhs & 15) as i32 + (rhs & 15) as i32 + carry as i32
                    };
                    let af = if subtract { nibble < 0 } else { nibble > 15 };
                    let expected = 2
                        | u32::from(cf)
                        | (u32::from(result.count_ones() % 2 == 0) << 2)
                        | (u32::from(af) << 4)
                        | (u32::from(result == 0) << 6)
                        | (u32::from(result >= 128) << 7)
                        | (u32::from(!(-128..=127).contains(&signed)) << 11);
                    assert_eq!(
                        materialize(0, kind, 8, lhs, rhs, result, carry, ARITHMETIC),
                        expected,
                        "{kind:?} {lhs:x} {rhs:x}"
                    );
                }
            }
        }
    }
}
