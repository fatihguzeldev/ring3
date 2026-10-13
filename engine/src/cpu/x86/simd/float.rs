use super::packed::{put, unsigned};
use super::{ConvertKind as C, FloatKind as F, SimdOperation};
use crate::math::{
    Rounding,
    ieee::{self, BinaryFormat, BinaryOperation},
};

fn rounding(mxcsr: u32) -> Rounding {
    match (mxcsr >> 13) & 3 {
        0 => Rounding::Nearest,
        1 => Rounding::Down,
        2 => Rounding::Up,
        _ => Rounding::Zero,
    }
}
fn ieee_format(width: usize) -> BinaryFormat {
    if width == 4 {
        BinaryFormat::Binary32
    } else {
        BinaryFormat::Binary64
    }
}
fn ieee_operation(kind: F) -> BinaryOperation {
    match kind {
        F::Add => BinaryOperation::Add,
        F::Subtract => BinaryOperation::Subtract,
        F::Multiply => BinaryOperation::Multiply,
        F::Divide => BinaryOperation::Divide,
        _ => unreachable!(),
    }
}
fn lane_exceptions(mut flags: u32, mxcsr: u32) -> u32 {
    if flags & !(mxcsr >> 7) & 0x18 != 0 {
        flags &= !0x20;
    }
    flags
}
fn instruction_exceptions(flags: u32, mxcsr: u32) -> u32 {
    if flags & !(mxcsr >> 7) & 7 != 0 {
        flags & 7
    } else {
        flags
    }
}
fn format(width: usize) -> (u64, u64, u64, u32, i32) {
    ieee_format(width).parameters()
}
fn nan(bits: u64, width: usize) -> bool {
    let (_, e, f, _, _) = format(width);
    bits & e == e && bits & f != 0
}
fn signaling(bits: u64, width: usize) -> bool {
    let (_, _, _, p, _) = format(width);
    nan(bits, width) && bits & (1 << (p - 1)) == 0
}
fn denormal(bits: u64, width: usize) -> bool {
    let (_, e, f, _, _) = format(width);
    bits & e == 0 && bits & f != 0
}
fn quiet(bits: u64, width: usize) -> u64 {
    bits | (1 << (format(width).3 - 1))
}
fn indefinite(width: usize) -> u64 {
    let (s, e, _, p, _) = format(width);
    s | e | (1 << (p - 1))
}
fn input(bits: u64, width: usize, mxcsr: u32) -> (u64, u32) {
    if denormal(bits, width) {
        if mxcsr & 0x40 != 0 {
            (bits & format(width).0, 0)
        } else {
            (bits, 2)
        }
    } else {
        (bits, 0)
    }
}
fn less(a: u64, b: u64, width: usize) -> bool {
    let sign = format(width).0;
    if a & !sign == 0 && b & !sign == 0 {
        return false;
    }
    if (a ^ b) & sign != 0 {
        return a & sign != 0;
    }
    if a & sign != 0 { a > b } else { a < b }
}
fn equal(a: u64, b: u64, width: usize) -> bool {
    a == b || (a | b) & !format(width).0 == 0
}
fn unbounded_tiny(kind: F, a: u64, b: u64, width: usize, round: Rounding) -> bool {
    matches!(kind, F::Add | F::Subtract | F::Multiply | F::Divide)
        && ieee::unbounded_tiny(ieee_operation(kind), ieee_format(width), a, b, round)
}

fn square_root(bits: u64, width: usize, round: Rounding) -> (u64, u32) {
    let result = ieee::square_root(bits, ieee_format(width), round);
    (result.bits, u32::from(result.exceptions))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CompareMode {
    Predicate(u8),
    Flags { quiet: bool },
}

fn compare_lane(
    mode: CompareMode,
    a: u64,
    b: u64,
    width: usize,
    mxcsr: u32,
) -> (u64, u32, Option<u32>) {
    let (a, ad) = input(a, width, mxcsr);
    let (b, bd) = input(b, width, mxcsr);
    let unordered = nan(a, width) || nan(b, width);
    let signaling_predicate = match mode {
        CompareMode::Predicate(immediate) => matches!(
            immediate & 31,
            1 | 2 | 5 | 6 | 9 | 10 | 13 | 14 | 16 | 19 | 20 | 23 | 24 | 27 | 28 | 31
        ),
        CompareMode::Flags { quiet } => !quiet,
    };
    let invalid = signaling(a, width) || signaling(b, width) || unordered && signaling_predicate;
    let exceptions = if invalid { 1 } else { ad | bd };
    let eq = !unordered && equal(a, b, width);
    let lt = !unordered && less(a, b, width);
    if matches!(mode, CompareMode::Flags { .. }) {
        return (
            0,
            exceptions,
            Some(if unordered {
                0x45
            } else if lt {
                1
            } else if eq {
                0x40
            } else {
                0
            }),
        );
    }
    let CompareMode::Predicate(immediate) = mode else {
        unreachable!()
    };
    let result = match immediate & 15 {
        0 => eq,
        1 => lt,
        2 => lt || eq,
        3 => unordered,
        4 => !eq,
        5 => !lt,
        6 => !(lt || eq),
        7 => !unordered,
        8 => eq || unordered,
        9 => lt || unordered,
        10 => lt || eq || unordered,
        11 => false,
        12 => !eq && !unordered,
        13 => !lt && !unordered,
        14 => !(lt || eq) && !unordered,
        15 => true,
        _ => unreachable!(),
    };
    (if result { u64::MAX } else { 0 }, exceptions, None)
}

#[cfg(test)]
pub(crate) fn compare_lanes<const N: usize>(
    mode: CompareMode,
    width: usize,
    count: usize,
    first: [u8; N],
    second: [u8; N],
    mxcsr: u32,
) -> ([u8; N], u32, Option<u32>) {
    assert!(matches!(width, 4 | 8) && count <= N / width);
    assert!(!matches!(mode, CompareMode::Flags { .. }) || count == 1);
    let mut result = [0; N];
    let mut exceptions = 0;
    let mut flags = None;
    for index in 0..count {
        let offset = index * width;
        let read = |source: &[u8; N]| {
            let mut bytes = [0; 8];
            bytes[..width].copy_from_slice(&source[offset..offset + width]);
            u64::from_le_bytes(bytes)
        };
        let (value, status, compare) =
            compare_lane(mode, read(&first), read(&second), width, mxcsr);
        exceptions |= lane_exceptions(status, mxcsr);
        if compare.is_some() {
            flags = compare;
        }
        result[offset..offset + width].copy_from_slice(&value.to_le_bytes()[..width]);
    }
    (result, instruction_exceptions(exceptions, mxcsr), flags)
}

fn binary(
    kind: F,
    mut a: u64,
    mut b: u64,
    width: usize,
    mxcsr: u32,
    predicate: u8,
) -> (u64, u32, Option<u32>) {
    if matches!(kind, F::Compare | F::CompareFlags | F::CompareFlagsQuiet) {
        let mode = match kind {
            F::Compare => CompareMode::Predicate(predicate & 7),
            F::CompareFlags => CompareMode::Flags { quiet: false },
            F::CompareFlagsQuiet => CompareMode::Flags { quiet: true },
            _ => unreachable!(),
        };
        return compare_lane(mode, a, b, width, mxcsr);
    }
    let unary = matches!(
        kind,
        F::SquareRoot | F::Reciprocal | F::ReciprocalSquareRoot
    );
    let (aa, ad) = input(a, width, mxcsr);
    a = aa;
    let (bb, bd) = input(b, width, mxcsr);
    b = bb;
    let mut flags = if unary { bd } else { ad | bd };
    let an = nan(a, width);
    let bn = nan(b, width);
    let sn = signaling(a, width) || signaling(b, width);
    if matches!(kind, F::Minimum | F::Maximum) {
        if an || bn {
            flags = 1;
        }
        let take_a = !an
            && !bn
            && !equal(a, b, width)
            && if kind == F::Minimum {
                less(a, b, width)
            } else {
                less(b, a, width)
            };
        return (if take_a { a } else { b }, flags, None);
    }
    if unary {
        if bn {
            return (
                quiet(b, width),
                if matches!(kind, F::Reciprocal | F::ReciprocalSquareRoot) {
                    0
                } else {
                    u32::from(signaling(b, width))
                },
                None,
            );
        }
    } else if an || bn {
        return (quiet(if an { a } else { b }, width), u32::from(sn), None);
    }
    if matches!(kind, F::Add | F::Subtract)
        && let Some(result) = ieee::exact_normal_sum(a, b, ieee_format(width), kind == F::Subtract)
    {
        return (result, 0, None);
    }
    if kind == F::Multiply
        && let Some(result) = ieee::exact_normal_product(a, b, ieee_format(width))
    {
        return (result, 0, None);
    }
    let round = rounding(mxcsr);
    let (mut result, mut status) = if kind == F::SquareRoot {
        square_root(b, width, round)
    } else if matches!(kind, F::Reciprocal | F::ReciprocalSquareRoot) {
        // exact nearest results satisfy the ISA approximation error bound; these instructions ignore RC and do not report SIMD exceptions.
        let b = if denormal(b, width) {
            b & format(width).0
        } else {
            b
        };
        let denominator = if kind == F::ReciprocalSquareRoot {
            square_root(b, width, Rounding::Nearest).0
        } else {
            b
        };
        let value = if width == 4 {
            ieee::binary(
                BinaryOperation::Divide,
                BinaryFormat::Binary32,
                0x3f80_0000,
                denominator,
                Rounding::Nearest,
            )
            .bits
        } else {
            unreachable!()
        };
        return (
            if denormal(value, width) {
                value & format(width).0
            } else {
                value
            },
            0,
            None,
        );
    } else {
        let result = ieee::binary(ieee_operation(kind), ieee_format(width), a, b, round);
        (result.bits, u32::from(result.exceptions))
    };
    if status & 1 != 0 {
        result = indefinite(width);
    }
    if status & 5 != 0 {
        flags = 0;
    }
    let minimum_normal = if width == 4 {
        0x0080_0000
    } else {
        0x0010_0000_0000_0000
    };
    let tiny =
        result & !format(width).0 <= minimum_normal && unbounded_tiny(kind, a, b, width, round);
    if tiny {
        if mxcsr & 0x800 == 0 || status & 0x20 != 0 {
            status |= 0x10;
        }
        if mxcsr & 0x800 != 0 && mxcsr & 0x8000 != 0 {
            result &= format(width).0;
            status |= 0x30;
        }
    }
    (result, status | flags, None)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ElementwiseKind {
    Add,
    Subtract,
    Multiply,
    Divide,
    Minimum,
    Maximum,
    SquareRoot,
    Reciprocal,
    ReciprocalSquareRoot,
}

impl ElementwiseKind {
    pub(crate) fn unary(self) -> bool {
        self == Self::SquareRoot || self.approximate()
    }

    pub(crate) fn approximate(self) -> bool {
        matches!(self, Self::Reciprocal | Self::ReciprocalSquareRoot)
    }

    fn from_legacy(kind: F) -> Option<Self> {
        Some(match kind {
            F::Add => Self::Add,
            F::Subtract => Self::Subtract,
            F::Multiply => Self::Multiply,
            F::Divide => Self::Divide,
            F::Minimum => Self::Minimum,
            F::Maximum => Self::Maximum,
            F::SquareRoot => Self::SquareRoot,
            F::Reciprocal => Self::Reciprocal,
            F::ReciprocalSquareRoot => Self::ReciprocalSquareRoot,
            _ => return None,
        })
    }

    fn legacy(self) -> F {
        match self {
            Self::Add => F::Add,
            Self::Subtract => F::Subtract,
            Self::Multiply => F::Multiply,
            Self::Divide => F::Divide,
            Self::Minimum => F::Minimum,
            Self::Maximum => F::Maximum,
            Self::SquareRoot => F::SquareRoot,
            Self::Reciprocal => F::Reciprocal,
            Self::ReciprocalSquareRoot => F::ReciprocalSquareRoot,
        }
    }
}

// all active lanes share one exception aggregation before any architectural publication.
pub(crate) fn elementwise<const N: usize>(
    kind: ElementwiseKind,
    width: usize,
    count: usize,
    first: [u8; N],
    second: [u8; N],
    mxcsr: u32,
) -> ([u8; N], u32) {
    assert!(matches!(width, 4 | 8) && count <= N / width);
    let mut result = first;
    let mut exceptions = 0;
    for lane in 0..count {
        let offset = lane * width;
        let read = |source: &[u8; N]| {
            let mut bits = [0; 8];
            bits[..width].copy_from_slice(&source[offset..offset + width]);
            u64::from_le_bytes(bits)
        };
        let left = if kind.unary() { 0 } else { read(&first) };
        let (value, status, _) = binary(kind.legacy(), left, read(&second), width, mxcsr, 0);
        exceptions |= lane_exceptions(status, mxcsr);
        result[offset..offset + width].copy_from_slice(&value.to_le_bytes()[..width]);
    }
    (result, instruction_exceptions(exceptions, mxcsr))
}

pub(crate) fn round_lanes<const N: usize>(
    width: usize,
    count: usize,
    mut out: [u8; N],
    b: [u8; N],
    mxcsr: u32,
    immediate: u8,
) -> ([u8; N], u32) {
    assert!(matches!(width, 4 | 8) && count <= N / width);
    let mode = if immediate & 4 != 0 {
        rounding(mxcsr)
    } else {
        rounding(u32::from(immediate & 3) << 13)
    };
    let mut exceptions = 0;
    for lane in 0..count {
        // round honors DAZ, but does not signal the denormal operand exception.
        let (bits, _) = input(unsigned(&b, lane * width, width), width, mxcsr);
        let result = ieee::round_to_integral(bits, ieee_format(width), mode);
        let status =
            u32::from(result.exceptions) & if immediate & 8 != 0 { !0x20 } else { u32::MAX };
        exceptions |= status;
        put(&mut out, lane * width, width, result.bits);
    }
    (out, instruction_exceptions(exceptions, mxcsr))
}

fn round_integral(op: SimdOperation, a: [u8; 16], b: [u8; 16], mxcsr: u32) -> ([u8; 16], u32) {
    let width = usize::from(op.lane_bytes);
    round_lanes(
        width,
        if op.scalar { 1 } else { 16 / width },
        a,
        b,
        mxcsr,
        op.immediate.unwrap(),
    )
}

fn dot_stage(status: u32, mxcsr: u32, exceptions: &mut u32) -> bool {
    let status = instruction_exceptions(status, mxcsr);
    *exceptions |= status;
    status & !(mxcsr >> 7) & 63 != 0
}

pub(crate) fn dot_product_lanes<const N: usize>(
    bytes: usize,
    width: usize,
    immediate: u8,
    a: [u8; N],
    b: [u8; N],
    mxcsr: u32,
) -> ([u8; N], u32) {
    assert!(bytes <= N && (width == 4 && matches!(bytes, 16 | 32) || width == 8 && bytes == 16));
    let block_lanes = 16 / width;
    let blocks = bytes / 16;
    let mut products = [0; 8];
    let mut product_exceptions = 0;
    for (lane, product) in products[..bytes / width].iter_mut().enumerate() {
        if immediate & (1 << (lane % block_lanes + 4)) != 0 {
            let (bits, status, _) = binary(
                F::Multiply,
                unsigned(&a, lane * width, width),
                unsigned(&b, lane * width, width),
                width,
                mxcsr,
                0,
            );
            *product = bits;
            product_exceptions |= lane_exceptions(status, mxcsr);
        }
    }
    let mut exceptions = 0;
    if dot_stage(product_exceptions, mxcsr, &mut exceptions) {
        return (a, exceptions);
    }
    let mut left = [0; 2];
    let mut right = [0; 2];
    let mut status = 0;
    for block in 0..blocks {
        let offset = block * block_lanes;
        let (bits, flags, _) = binary(
            F::Add,
            products[offset],
            products[offset + 1],
            width,
            mxcsr,
            0,
        );
        left[block] = bits;
        status |= lane_exceptions(flags, mxcsr);
        if width == 4 {
            let (bits, flags, _) = binary(
                F::Add,
                products[offset + 2],
                products[offset + 3],
                width,
                mxcsr,
                0,
            );
            right[block] = bits;
            status |= lane_exceptions(flags, mxcsr);
        }
    }
    if dot_stage(status, mxcsr, &mut exceptions) {
        return (a, exceptions);
    }
    if width == 4 {
        let mut status = 0;
        for block in 0..blocks {
            let (bits, flags, _) = binary(F::Add, left[block], right[block], width, mxcsr, 0);
            left[block] = bits;
            status |= lane_exceptions(flags, mxcsr);
        }
        if dot_stage(status, mxcsr, &mut exceptions) {
            return (a, exceptions);
        }
    }
    let mut out = [0; N];
    for lane in 0..bytes / width {
        if immediate & (1 << (lane % block_lanes)) != 0 {
            put(&mut out, lane * width, width, left[lane / block_lanes]);
        }
    }
    (out, exceptions)
}

pub(crate) fn dot_product(
    width: usize,
    immediate: u8,
    a: [u8; 16],
    b: [u8; 16],
    mxcsr: u32,
) -> ([u8; 16], u32) {
    dot_product_lanes(16, width, immediate, a, b, mxcsr)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PairwiseKind {
    AddSubtract,
    HorizontalAdd,
    HorizontalSubtract,
}

impl PairwiseKind {
    fn from_legacy(kind: F) -> Option<Self> {
        Some(match kind {
            F::AddSubtract => Self::AddSubtract,
            F::HorizontalAdd => Self::HorizontalAdd,
            F::HorizontalSubtract => Self::HorizontalSubtract,
            _ => return None,
        })
    }
}

pub(crate) fn pairwise_lanes<const N: usize>(
    kind: PairwiseKind,
    width: usize,
    bytes: usize,
    a: [u8; N],
    b: [u8; N],
    mxcsr: u32,
) -> ([u8; N], u32) {
    assert!(matches!(width, 4 | 8) && bytes <= N && bytes % 16 == 0);
    let count = bytes / width;
    let block_lanes = 16 / width;
    let mut out = a;
    let mut exceptions = 0;
    for lane in 0..count {
        let offset = lane * width;
        let (operation, left, right) = match kind {
            PairwiseKind::AddSubtract => (
                if lane % 2 == 0 { F::Subtract } else { F::Add },
                unsigned(&a, offset, width),
                unsigned(&b, offset, width),
            ),
            PairwiseKind::HorizontalAdd | PairwiseKind::HorizontalSubtract => {
                let within = lane % block_lanes;
                let source = if within < block_lanes / 2 { &a } else { &b };
                let offset = lane / block_lanes * 16 + within % (block_lanes / 2) * width * 2;
                (
                    if kind == PairwiseKind::HorizontalAdd {
                        F::Add
                    } else {
                        F::Subtract
                    },
                    unsigned(source, offset, width),
                    unsigned(source, offset + width, width),
                )
            }
        };
        let (value, status, _) = binary(operation, left, right, width, mxcsr, 0);
        exceptions |= lane_exceptions(status, mxcsr);
        put(&mut out, offset, width, value);
    }
    (out, instruction_exceptions(exceptions, mxcsr))
}

pub(super) fn floating(
    op: SimdOperation,
    a: [u8; 16],
    b: [u8; 16],
    mxcsr: u32,
    incoming_flags: u32,
) -> ([u8; 16], u32, u32) {
    let super::SimdKind::Float(kind) = op.kind else {
        unreachable!()
    };
    if matches!(kind, F::Round | F::DotProduct) {
        let (out, exceptions) = if kind == F::Round {
            round_integral(op, a, b, mxcsr)
        } else {
            dot_product(
                usize::from(op.lane_bytes),
                op.immediate.unwrap(),
                a,
                b,
                mxcsr,
            )
        };
        return (out, exceptions, incoming_flags);
    }
    let width = usize::from(op.lane_bytes);
    let count = if op.scalar {
        1
    } else {
        usize::from(op.vector_bytes) / width
    };
    if let Some(kind) = ElementwiseKind::from_legacy(kind) {
        let (out, exceptions) = elementwise(kind, width, count, a, b, mxcsr);
        return (out, exceptions, incoming_flags);
    }
    if let Some(kind) = PairwiseKind::from_legacy(kind) {
        let (out, exceptions) =
            pairwise_lanes(kind, width, usize::from(op.vector_bytes), a, b, mxcsr);
        return (out, exceptions, incoming_flags);
    }
    let mut out = a;
    let mut exceptions = 0;
    let mut flags = incoming_flags;
    for lane in 0..count {
        let offset = lane * width;
        let left = unsigned(&a, offset, width);
        let right = unsigned(&b, offset, width);
        let (value, status, compare) =
            binary(kind, left, right, width, mxcsr, op.immediate.unwrap_or(0));
        exceptions |= lane_exceptions(status, mxcsr);
        if let Some(compare) = compare {
            flags = (incoming_flags & !0x8d5) | compare;
        } else {
            put(&mut out, offset, width, value);
        }
    }
    (out, instruction_exceptions(exceptions, mxcsr), flags)
}

pub(crate) fn conversion_lanes<const N: usize>(
    kind: C,
    truncate: bool,
    count: usize,
    mut out: [u8; N],
    b: [u8; N],
    mxcsr: u32,
) -> ([u8; N], u32) {
    let (source_width, destination_width) = match kind {
        C::SignedDwordsToSingles
        | C::SignedDwordToSingle
        | C::SinglesToSignedDwords
        | C::SingleToSignedDword => (4, 4),
        C::SignedDwordsToDoubles
        | C::SignedDwordToDouble
        | C::SinglesToDoubles
        | C::SingleToDouble => (4, 8),
        C::DoublesToSignedDwords
        | C::DoubleToSignedDword
        | C::DoublesToSingles
        | C::DoubleToSingle => (8, 4),
    };
    assert!(count <= N / source_width && count <= N / destination_width);
    let round = if truncate {
        Rounding::Zero
    } else {
        rounding(mxcsr)
    };
    let mut flags = 0;
    for lane in 0..count {
        let raw = unsigned(&b, lane * source_width, source_width);
        let to_integer = matches!(
            kind,
            C::SinglesToSignedDwords
                | C::DoublesToSignedDwords
                | C::SingleToSignedDword
                | C::DoubleToSignedDword
        );
        let from_integer = matches!(
            kind,
            C::SignedDwordsToSingles
                | C::SignedDwordsToDoubles
                | C::SignedDwordToSingle
                | C::SignedDwordToDouble
        );
        let (value, status) = if from_integer {
            let result =
                ieee::from_signed32(raw as u32 as i32, ieee_format(destination_width), round);
            (result.bits, u32::from(result.exceptions))
        } else {
            let (raw, denormal_flag) = input(raw, source_width, mxcsr);
            if to_integer {
                let result = ieee::to_signed32(raw, ieee_format(source_width), round);
                (result.bits, u32::from(result.exceptions))
            } else if source_width == 4 {
                let result = ieee::convert(raw, BinaryFormat::Binary32, round);
                (
                    result.bits,
                    u32::from(result.exceptions) | if nan(raw, 4) { 0 } else { denormal_flag },
                )
            } else {
                let result = ieee::convert(raw, BinaryFormat::Binary64, round);
                let mut value = result.bits;
                let mut status =
                    u32::from(result.exceptions) | if nan(raw, 8) { 0 } else { denormal_flag };
                let tiny = ieee::narrowing_tiny(raw, round);
                if tiny {
                    if mxcsr & 0x800 == 0 || status & 0x20 != 0 {
                        status |= 0x10;
                    }
                    if mxcsr & 0x800 != 0 && mxcsr & 0x8000 != 0 {
                        value &= 0x8000_0000;
                        status |= 0x30;
                    }
                }
                (value, status)
            }
        };
        flags |= lane_exceptions(status, mxcsr);
        put(&mut out, lane * destination_width, destination_width, value);
    }
    (out, instruction_exceptions(flags, mxcsr))
}

pub(super) fn convert(
    op: SimdOperation,
    a: [u8; 16],
    b: [u8; 16],
    mxcsr: u32,
) -> ([u8; 16], u32, Option<u32>) {
    let super::SimdKind::Convert { kind, truncate } = op.kind else {
        unreachable!()
    };
    let mut out = if op.scalar { a } else { [0; 16] };
    let (_, _, count) = match kind {
        C::SignedDwordsToSingles => (
            4,
            4,
            if matches!(
                op.source,
                super::SimdOperand::Register(super::SimdRegister::Mmx(_))
            ) || op.memory_bytes == 8
            {
                2
            } else {
                4
            },
        ),
        C::SignedDwordsToDoubles => (4, 8, 2),
        C::SinglesToSignedDwords => (
            4,
            4,
            if matches!(
                op.destination,
                super::SimdOperand::Register(super::SimdRegister::Mmx(_))
            ) {
                2
            } else {
                4
            },
        ),
        C::DoublesToSignedDwords => (8, 4, 2),
        C::SinglesToDoubles => (4, 8, 2),
        C::DoublesToSingles => (8, 4, 2),
        C::SignedDwordToSingle => (4, 4, 1),
        C::SignedDwordToDouble => (4, 8, 1),
        C::SingleToSignedDword => (4, 4, 1),
        C::DoubleToSignedDword => (8, 4, 1),
        C::SingleToDouble => (4, 8, 1),
        C::DoubleToSingle => (8, 4, 1),
    };
    if kind == C::SignedDwordsToSingles && count == 2 {
        out = a;
    }
    let (out, flags) = conversion_lanes(kind, truncate, count, out, b, mxcsr);
    let gpr = matches!(kind, C::SingleToSignedDword | C::DoubleToSignedDword)
        .then(|| unsigned(&out, 0, 4) as u32);
    (out, flags, gpr)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn radical_has_exact_rounding_and_signed_zero() {
        assert_eq!(
            square_root(0x4080_0000, 4, Rounding::Nearest),
            (0x4000_0000, 0)
        );
        assert_eq!(
            square_root(0x4000_0000, 4, Rounding::Nearest),
            (0x3fb5_04f3, 32)
        );
        assert_eq!(square_root(0x8000_0000, 4, Rounding::Up), (0x8000_0000, 0));
        assert_eq!(
            square_root(0x3ff0_0000_0000_0000, 8, Rounding::Zero),
            (0x3ff0_0000_0000_0000, 0)
        );
    }
    #[test]
    fn nan_priority_and_compare_predicates_are_x86() {
        assert_eq!(
            binary(F::Add, 0x7f80_1234, 0x7fc0_5678, 4, 0x1f80, 0).0,
            0x7fc0_1234
        );
        assert_eq!(
            binary(F::Minimum, 0x7fc0_1234, 0x3f80_0000, 4, 0x1f80, 0).0,
            0x3f80_0000
        );
        assert_eq!(binary(F::Compare, 0x7fc0_1234, 0, 4, 0x1f80, 0).1, 0);
        assert_eq!(binary(F::Compare, 0x7fc0_1234, 0, 4, 0x1f80, 1).1, 1);
    }
    #[test]
    fn legacy_elementwise_wrapper_retains_scalar_merge_and_flags() {
        let op = SimdOperation {
            kind: super::super::SimdKind::Float(F::Add),
            destination: super::super::SimdOperand::Register(super::super::SimdRegister::Xmm(0)),
            source: super::super::SimdOperand::Register(super::super::SimdRegister::Xmm(1)),
            vector_bytes: 16,
            lane_bytes: 4,
            memory_bytes: 0,
            immediate: None,
            aligned: false,
            scalar: true,
            implicit_address: None,
        };
        let mut first = [0xa5; 16];
        first[..4].copy_from_slice(&0x4000_0000u32.to_le_bytes());
        let mut second = [0x5a; 16];
        second[..4].copy_from_slice(&0x4080_0000u32.to_le_bytes());
        let mut expected = first;
        expected[..4].copy_from_slice(&0x40c0_0000u32.to_le_bytes());
        assert_eq!(
            floating(op, first, second, 0x1f80, 0x8d7),
            (expected, 0, 0x8d7)
        );
    }

    #[test]
    fn legacy_elementwise_wrapper_aggregates_packed_exception_priority() {
        let op = SimdOperation {
            kind: super::super::SimdKind::Float(F::Add),
            destination: super::super::SimdOperand::Register(super::super::SimdRegister::Xmm(0)),
            source: super::super::SimdOperand::Register(super::super::SimdRegister::Xmm(1)),
            vector_bytes: 16,
            lane_bytes: 4,
            memory_bytes: 0,
            immediate: None,
            aligned: false,
            scalar: false,
            implicit_address: None,
        };
        let mut first = [0; 16];
        let mut second = [0; 16];
        for (index, (left, right)) in [
            (0x4b80_0000u32, 0x3f80_0000u32),
            (1, 0x3f80_0000),
            (0x7f80_0000, 0xff80_0000),
            (0x4000_0000, 0x4080_0000),
        ]
        .into_iter()
        .enumerate()
        {
            first[index * 4..index * 4 + 4].copy_from_slice(&left.to_le_bytes());
            second[index * 4..index * 4 + 4].copy_from_slice(&right.to_le_bytes());
        }
        let (_, exceptions, flags) = floating(op, first, second, 0x1f00, 0x8d7);
        assert_eq!(exceptions, 3);
        assert_eq!(flags, 0x8d7);
    }
}
