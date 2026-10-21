use super::{Ext80, Rounded, X87Rounding};
use crate::math::PRECISION;

#[inline]
fn operands(a: Ext80, b: Ext80, control: u16) -> Option<(u64, u64, i32, i32)> {
    let ae = a.exponent_bits();
    let be = b.exponent_bits();
    let av = a.significand();
    let bv = b.significand();
    if control & 0x0300 != 0
        || ae == 0
        || ae == 0x7fff
        || be == 0
        || be == 0x7fff
        || av >> 63 == 0
        || bv >> 63 == 0
        || (av | bv) & ((1_u64 << 40) - 1) != 0
    {
        return None;
    }
    Some((
        av >> 40,
        bv >> 40,
        i32::from(ae) - 16383,
        i32::from(be) - 16383,
    ))
}

#[inline]
fn shift_jam(value: u64, shift: u32) -> u64 {
    if shift == 0 {
        value
    } else if shift >= 64 {
        u64::from(value != 0)
    } else {
        (value >> shift) | u64::from(value & ((1_u64 << shift) - 1) != 0)
    }
}

#[inline]
fn rounded(negative: bool, mut exponent: i32, mut extended: u64, control: u16) -> Option<Rounded> {
    if extended == 0 {
        return Some(Rounded::exact(Ext80::from_parts(negative, 0, 0), 0));
    }
    let high = 63 - extended.leading_zeros();
    if high > 26 {
        extended = shift_jam(extended, high - 26);
        exponent += (high - 26) as i32;
    } else if high < 26 {
        extended <<= 26 - high;
        exponent -= (26 - high) as i32;
    }
    if !(-16382..16383).contains(&exponent) {
        return None;
    }
    let retained = extended >> 3;
    let discarded = extended & 7;
    let incremented = super::rounding_increment(
        retained,
        discarded,
        4,
        negative,
        X87Rounding::from_control(control),
    );
    let significand = retained + u64::from(incremented);
    let carry = significand >= 1 << 24;
    exponent += i32::from(carry);
    Some(Rounded {
        value: Ext80::from_parts(
            negative,
            (exponent + 16383) as u16,
            if carry { 1 << 63 } else { significand << 40 },
        ),
        exceptions: if discarded != 0 { PRECISION } else { 0 },
        incremented,
    })
}

#[inline]
pub(super) fn add(a: Ext80, b: Ext80, control: u16) -> Option<Rounded> {
    let (av, bv, ae, be) = operands(a, b, control)?;
    let exponent = ae.max(be);
    let av = shift_jam(av << 3, (exponent - ae) as u32);
    let bv = shift_jam(bv << 3, (exponent - be) as u32);
    if a.negative() == b.negative() {
        rounded(a.negative(), exponent, av + bv, control)
    } else if av == bv {
        Some(Rounded::exact(
            Ext80::from_parts(
                X87Rounding::from_control(control) == X87Rounding::Down,
                0,
                0,
            ),
            0,
        ))
    } else if av > bv {
        rounded(a.negative(), exponent, av - bv, control)
    } else {
        rounded(b.negative(), exponent, bv - av, control)
    }
}

#[inline]
pub(super) fn multiply(a: Ext80, b: Ext80, control: u16) -> Option<Rounded> {
    let (av, bv, ae, be) = operands(a, b, control)?;
    let product = av * bv;
    let high = 63 - product.leading_zeros();
    rounded(
        a.negative() ^ b.negative(),
        ae + be + high as i32 - 46,
        shift_jam(product, high - 26),
        control,
    )
}

#[inline]
fn quotient_parts(bits: u32, av: u64, bv: u64) -> (u64, i32, core::cmp::Ordering) {
    let exponent = ((bits >> 23) & 255) as i32 - 127;
    let significand = u64::from((bits & 0x7f_ffff) | 0x80_0000);
    let product = significand * bv;
    let numerator = av << (23 - exponent);
    (significand, exponent, product.cmp(&numerator))
}

#[inline]
pub(super) fn divide(a: Ext80, b: Ext80, control: u16) -> Option<Rounded> {
    let (av, bv, ae, be) = operands(a, b, control)?;
    let negative = a.negative() ^ b.negative();
    // a 24-bit rational cannot lie within binary64 rounding error of a binary32 midpoint.
    let mut bits = ((av as f64 / bv as f64) as f32).to_bits();
    let (_, _, order) = quotient_parts(bits, av, bv);
    use core::cmp::Ordering::{Equal, Greater, Less};
    let mode = X87Rounding::from_control(control);
    if mode != X87Rounding::Nearest && order != Equal {
        let upward = mode == X87Rounding::Up && !negative || mode == X87Rounding::Down && negative;
        if upward && order == Less {
            bits += 1;
        } else if !upward && order == Greater {
            bits -= 1;
        }
    }
    let (significand, quotient_exponent, order) = quotient_parts(bits, av, bv);
    let exponent = ae - be + quotient_exponent;
    if !(-16382..16383).contains(&exponent) {
        return None;
    }
    Some(Rounded {
        value: Ext80::from_parts(negative, (exponent + 16383) as u16, significand << 40),
        exceptions: if order != Equal { PRECISION } else { 0 },
        incremented: order == Greater,
    })
}
