use super::{
    format, ieee, ieee_format, indefinite, input, instruction_exceptions, lane_exceptions, nan,
    put, quiet, rounding, signaling, unsigned,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FusedKind {
    Add,
    Subtract,
    NegativeAdd,
    NegativeSubtract,
    AddSubtract,
    SubtractAdd,
}

impl FusedKind {
    fn signs(self, lane: usize) -> (bool, bool) {
        let negative_product = matches!(self, Self::NegativeAdd | Self::NegativeSubtract);
        let subtract = match self {
            Self::Subtract | Self::NegativeSubtract => true,
            Self::AddSubtract => lane.is_multiple_of(2),
            Self::SubtractAdd => !lane.is_multiple_of(2),
            _ => false,
        };
        (negative_product, subtract)
    }

    pub(crate) fn alternating(self) -> bool {
        matches!(self, Self::AddSubtract | Self::SubtractAdd)
    }
}

fn lane(
    kind: FusedKind,
    index: usize,
    a: u64,
    b: u64,
    c: u64,
    width: usize,
    mxcsr: u32,
) -> (u64, u32) {
    let (a, ad) = input(a, width, mxcsr);
    let (b, bd) = input(b, width, mxcsr);
    let (c, cd) = input(c, width, mxcsr);
    // nan selection uses the encoded multiplicand order before arithmetic sign changes.
    if let Some(value) = [a, b, c].into_iter().find(|value| nan(*value, width)) {
        let invalid = signaling(a, width) || signaling(b, width) || signaling(c, width);
        return (quiet(value, width), u32::from(invalid));
    }
    let (negative_product, subtract) = kind.signs(index);
    let sign = format(width).0;
    let a = a ^ if negative_product { sign } else { 0 };
    let c = c ^ if subtract { sign } else { 0 };
    let round = rounding(mxcsr);
    let computed = ieee::fused_multiply_add(ieee_format(width), a, b, c, round);
    let mut result = computed.bits;
    let mut status = u32::from(computed.exceptions);
    let mut input_flags = ad | bd | cd;
    if status & 1 != 0 {
        result = indefinite(width);
        input_flags = 0;
    }
    let minimum_normal = if width == 4 {
        0x0080_0000
    } else {
        0x0010_0000_0000_0000
    };
    if result & !sign <= minimum_normal
        && ieee::fused_unbounded_tiny(ieee_format(width), a, b, c, round)
    {
        if mxcsr & 0x800 == 0 || status & 0x20 != 0 {
            status |= 0x10;
        }
        if mxcsr & 0x8800 == 0x8800 {
            result &= sign;
            status |= 0x30;
        }
    }
    debug_assert!(status & 4 == 0);
    (result, status | input_flags)
}

pub(crate) fn fused_lanes<const N: usize>(
    kind: FusedKind,
    width: usize,
    count: usize,
    a: [u8; N],
    b: [u8; N],
    c: [u8; N],
    mxcsr: u32,
) -> ([u8; N], u32) {
    let mut result = [0; N];
    let mut exceptions = 0;
    for index in 0..count {
        let offset = index * width;
        let (value, flags) = lane(
            kind,
            index,
            unsigned(&a, offset, width),
            unsigned(&b, offset, width),
            unsigned(&c, offset, width),
            width,
            mxcsr,
        );
        put(&mut result, offset, width, value);
        exceptions |= lane_exceptions(flags, mxcsr);
    }
    (result, instruction_exceptions(exceptions, mxcsr))
}
