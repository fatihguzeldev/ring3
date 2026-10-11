use core::cmp::Ordering;

pub const INVALID: u16 = 1;
pub const DENORMAL: u16 = 2;
pub const ZERO_DIVIDE: u16 = 4;
pub const OVERFLOW: u16 = 8;
pub const UNDERFLOW: u16 = 16;
pub const PRECISION: u16 = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum X87Rounding {
    Nearest,
    Down,
    Up,
    Zero,
}

impl X87Rounding {
    pub fn from_control(control: u16) -> Self {
        match (control >> 10) & 3 {
            0 => Self::Nearest,
            1 => Self::Down,
            2 => Self::Up,
            _ => Self::Zero,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ext80(pub [u8; 10]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Zero,
    Finite { significand: u64, exponent: i32 },
    Infinity,
    NaN { signaling: bool },
    Unsupported,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Rounded {
    pub value: Ext80,
    pub exceptions: u16,
    pub incremented: bool,
}

impl Ext80 {
    pub const ZERO: Self = Self([0; 10]);
    pub const ONE: Self = Self([0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f]);
    pub const INDEFINITE: Self = Self([0, 0, 0, 0, 0, 0, 0, 0xc0, 0xff, 0xff]);

    pub fn from_parts(negative: bool, exponent: u16, significand: u64) -> Self {
        let mut bytes = [0; 10];
        bytes[..8].copy_from_slice(&significand.to_le_bytes());
        bytes[8..].copy_from_slice(&(exponent | (u16::from(negative) << 15)).to_le_bytes());
        Self(bytes)
    }

    pub fn negative(self) -> bool {
        self.0[9] & 0x80 != 0
    }

    pub fn exponent_bits(self) -> u16 {
        u16::from_le_bytes([self.0[8], self.0[9]]) & 0x7fff
    }

    pub fn significand(self) -> u64 {
        u64::from_le_bytes(self.0[..8].try_into().expect("fixed significand"))
    }

    pub fn negated(mut self) -> Self {
        self.0[9] ^= 0x80;
        self
    }

    pub fn absolute(mut self) -> Self {
        self.0[9] &= 0x7f;
        self
    }

    pub(crate) fn kind(self) -> Kind {
        let exponent = self.exponent_bits();
        let significand = self.significand();
        if exponent == 0 && significand == 0 {
            return Kind::Zero;
        }
        if exponent != 0 && significand >> 63 == 0 {
            return Kind::Unsupported;
        }
        if exponent == 0x7fff {
            return if significand == 1 << 63 {
                Kind::Infinity
            } else {
                Kind::NaN {
                    signaling: significand & (1 << 62) == 0,
                }
            };
        }
        let shift = significand.leading_zeros();
        Kind::Finite {
            significand: significand << shift,
            exponent: i32::from(exponent.max(1)) - 16383 - shift as i32,
        }
    }

    pub(crate) fn operand_exceptions(self) -> u16 {
        if self.exponent_bits() == 0 && self.significand() != 0 && self.significand() >> 63 == 0 {
            DENORMAL
        } else {
            0
        }
    }

    pub(crate) fn tag(self) -> u16 {
        match self.kind() {
            Kind::Zero => 1,
            Kind::Finite { .. } if self.exponent_bits() != 0 => 0,
            _ => 2,
        }
    }

    fn quiet(self) -> Self {
        Self::from_parts(self.negative(), 0x7fff, self.significand() | (3 << 62))
    }

    pub(super) fn special(a: Self, b: Self) -> Option<Rounded> {
        let ak = a.kind();
        let bk = b.kind();
        if matches!(ak, Kind::Unsupported) || matches!(bk, Kind::Unsupported) {
            return Some(Rounded::invalid());
        }
        if matches!(ak, Kind::NaN { .. }) || matches!(bk, Kind::NaN { .. }) {
            let signaling = matches!(ak, Kind::NaN { signaling: true })
                || matches!(bk, Kind::NaN { signaling: true });
            let value = match (ak, bk) {
                (Kind::NaN { signaling: false }, Kind::NaN { signaling: true }) => a,
                (Kind::NaN { signaling: true }, Kind::NaN { signaling: false }) => b,
                (Kind::NaN { .. }, Kind::NaN { .. }) => {
                    if a.significand() >= b.significand() {
                        a
                    } else {
                        b
                    }
                }
                (Kind::NaN { .. }, _) => a,
                _ => b,
            };
            return Some(Rounded {
                value: value.quiet(),
                exceptions: if signaling { INVALID } else { 0 },
                incremented: false,
            });
        }
        None
    }

    pub(crate) fn add(self, rhs: Self, control: u16) -> Rounded {
        if let Some(result) = Self::special(self, rhs) {
            return result;
        }
        let mode = X87Rounding::from_control(control);
        let exceptions = self.operand_exceptions() | rhs.operand_exceptions();
        match (self.kind(), rhs.kind()) {
            (Kind::Infinity, Kind::Infinity) if self.negative() != rhs.negative() => {
                Rounded::invalid()
            }
            (Kind::Infinity, _) => Rounded::exact(self, exceptions),
            (_, Kind::Infinity) => Rounded::exact(rhs, exceptions),
            (Kind::Zero, Kind::Zero) => Rounded::exact(
                Self::from_parts(
                    if self.negative() == rhs.negative() {
                        self.negative()
                    } else {
                        mode == X87Rounding::Down
                    },
                    0,
                    0,
                ),
                exceptions,
            ),
            (
                Kind::Zero,
                Kind::Finite {
                    significand,
                    exponent,
                },
            ) => round(
                rhs.negative(),
                exponent,
                u128::from(significand) << 3,
                control,
                exceptions,
            ),
            (
                Kind::Finite {
                    significand,
                    exponent,
                },
                Kind::Zero,
            ) => round(
                self.negative(),
                exponent,
                u128::from(significand) << 3,
                control,
                exceptions,
            ),
            (
                Kind::Finite {
                    significand: a,
                    exponent: ae,
                },
                Kind::Finite {
                    significand: b,
                    exponent: be,
                },
            ) => {
                let exponent = ae.max(be);
                let av = shift_jam(u128::from(a) << 3, (exponent - ae) as u32);
                let bv = shift_jam(u128::from(b) << 3, (exponent - be) as u32);
                if self.negative() == rhs.negative() {
                    round(self.negative(), exponent, av + bv, control, exceptions)
                } else if av == bv {
                    Rounded::exact(
                        Self::from_parts(mode == X87Rounding::Down, 0, 0),
                        exceptions,
                    )
                } else if av > bv {
                    round(self.negative(), exponent, av - bv, control, exceptions)
                } else {
                    round(rhs.negative(), exponent, bv - av, control, exceptions)
                }
            }
            _ => Rounded::invalid(),
        }
    }

    pub(crate) fn multiply(self, rhs: Self, control: u16) -> Rounded {
        if let Some(result) = Self::special(self, rhs) {
            return result;
        }
        let negative = self.negative() ^ rhs.negative();
        let exceptions = self.operand_exceptions() | rhs.operand_exceptions();
        match (self.kind(), rhs.kind()) {
            (Kind::Infinity, Kind::Zero) | (Kind::Zero, Kind::Infinity) => Rounded::invalid(),
            (Kind::Infinity, _) | (_, Kind::Infinity) => {
                Rounded::exact(Self::from_parts(negative, 0x7fff, 1 << 63), exceptions)
            }
            (Kind::Zero, _) | (_, Kind::Zero) => {
                Rounded::exact(Self::from_parts(negative, 0, 0), exceptions)
            }
            (
                Kind::Finite {
                    significand: a,
                    exponent: ae,
                },
                Kind::Finite {
                    significand: b,
                    exponent: be,
                },
            ) => {
                let product = u128::from(a) * u128::from(b);
                let high = 127 - product.leading_zeros();
                round(
                    negative,
                    ae + be + high as i32 - 126,
                    shift_jam(product, high - 66),
                    control,
                    exceptions,
                )
            }
            _ => Rounded::invalid(),
        }
    }

    pub(crate) fn divide(self, rhs: Self, control: u16) -> Rounded {
        if let Some(result) = Self::special(self, rhs) {
            return result;
        }
        let negative = self.negative() ^ rhs.negative();
        let exceptions = self.operand_exceptions() | rhs.operand_exceptions();
        match (self.kind(), rhs.kind()) {
            (Kind::Infinity, Kind::Infinity) | (Kind::Zero, Kind::Zero) => Rounded::invalid(),
            (_, Kind::Zero) => Rounded::exact(
                Self::from_parts(negative, 0x7fff, 1 << 63),
                exceptions | ZERO_DIVIDE,
            ),
            (Kind::Infinity, _) => {
                Rounded::exact(Self::from_parts(negative, 0x7fff, 1 << 63), exceptions)
            }
            (Kind::Zero, _) | (_, Kind::Infinity) => {
                Rounded::exact(Self::from_parts(negative, 0, 0), exceptions)
            }
            (
                Kind::Finite {
                    significand: a,
                    exponent: ae,
                },
                Kind::Finite {
                    significand: b,
                    exponent: be,
                },
            ) => {
                let below = a < b;
                let numerator = u128::from(a) << if below { 64 } else { 63 };
                let divisor = u128::from(b);
                let mut quotient = numerator / divisor;
                let mut remainder = numerator % divisor;
                for _ in 0..3 {
                    remainder <<= 1;
                    quotient = (quotient << 1) | u128::from(remainder >= divisor);
                    if remainder >= divisor {
                        remainder -= divisor;
                    }
                }
                quotient |= u128::from(remainder != 0);
                round(
                    negative,
                    ae - be - i32::from(below),
                    quotient,
                    control,
                    exceptions,
                )
            }
            _ => Rounded::invalid(),
        }
    }

    pub(crate) fn sqrt(self, control: u16) -> Rounded {
        if let Some(result) = Self::special(self, Self::ZERO) {
            return result;
        }
        match self.kind() {
            Kind::Zero => Rounded::exact(self, 0),
            _ if self.negative() => Rounded::invalid(),
            Kind::Infinity => Rounded::exact(self, 0),
            Kind::Finite {
                significand,
                exponent,
            } => {
                let odd = exponent & 1 != 0;
                let radicand = u128::from(significand) << if odd { 64 } else { 63 };
                let mut root = integer_sqrt(radicand);
                let mut remainder = radicand - root * root;
                for _ in 0..3 {
                    remainder <<= 2;
                    let trial = (root << 2) | 1;
                    root <<= 1;
                    if remainder >= trial {
                        remainder -= trial;
                        root |= 1;
                    }
                }
                root |= u128::from(remainder != 0);
                round(
                    false,
                    exponent.div_euclid(2),
                    root,
                    control,
                    self.operand_exceptions(),
                )
            }
            _ => Rounded::invalid(),
        }
    }

    pub(crate) fn compare(self, rhs: Self, quiet: bool) -> (Option<Ordering>, u16) {
        let exceptions = self.operand_exceptions() | rhs.operand_exceptions();
        if matches!(self.kind(), Kind::Unsupported) || matches!(rhs.kind(), Kind::Unsupported) {
            return (None, INVALID);
        }
        if matches!(self.kind(), Kind::NaN { .. }) || matches!(rhs.kind(), Kind::NaN { .. }) {
            let invalid = !quiet
                || matches!(self.kind(), Kind::NaN { signaling: true })
                || matches!(rhs.kind(), Kind::NaN { signaling: true });
            return (None, if invalid { INVALID } else { 0 });
        }
        if matches!(self.kind(), Kind::Zero) && matches!(rhs.kind(), Kind::Zero) {
            return (Some(Ordering::Equal), exceptions);
        }
        if self.negative() != rhs.negative() {
            return (
                Some(if self.negative() {
                    Ordering::Less
                } else {
                    Ordering::Greater
                }),
                exceptions,
            );
        }
        let order = (self.exponent_bits().max(1), self.significand())
            .cmp(&(rhs.exponent_bits().max(1), rhs.significand()));
        (
            Some(if self.negative() {
                order.reverse()
            } else {
                order
            }),
            exceptions,
        )
    }

    pub fn from_i64(value: i64) -> Self {
        let magnitude = value.unsigned_abs();
        if magnitude == 0 {
            return Self::ZERO;
        }
        let shift = magnitude.leading_zeros();
        Self::from_parts(value < 0, (16383 + 63 - shift) as u16, magnitude << shift)
    }

    pub(crate) fn remainder(self, rhs: Self, nearest: bool) -> (Rounded, bool, u8) {
        if let Some(result) = Self::special(self, rhs) {
            return (result, false, 0);
        }
        if matches!(self.kind(), Kind::Infinity) || matches!(rhs.kind(), Kind::Zero) {
            return (Rounded::invalid(), false, 0);
        }
        if matches!(self.kind(), Kind::Zero) || matches!(rhs.kind(), Kind::Infinity) {
            return (
                Rounded::exact(self, self.operand_exceptions() | rhs.operand_exceptions()),
                false,
                0,
            );
        }
        let (
            Kind::Finite {
                significand: a,
                exponent: ae,
            },
            Kind::Finite {
                significand: b,
                exponent: be,
            },
        ) = (self.kind(), rhs.kind())
        else {
            return (Rounded::invalid(), false, 0);
        };
        let difference = ae - be;
        let partial = difference >= 64;
        let reduction = if partial { difference - 32 } else { 0 };
        let relative = difference - reduction;
        let (numerator, divisor, base) = if relative >= 0 {
            (
                u128::from(a) << relative as u32,
                u128::from(b),
                be + reduction,
            )
        } else if relative >= -1 {
            (u128::from(a), u128::from(b) << (-relative) as u32, ae)
        } else {
            return (
                Rounded::exact(self, self.operand_exceptions() | rhs.operand_exceptions()),
                false,
                0,
            );
        };
        let mut quotient = numerator / divisor;
        let mut remainder = numerator % divisor;
        let round_up = nearest
            && !partial
            && (remainder * 2 > divisor || (remainder * 2 == divisor && quotient & 1 != 0));
        let negative = if round_up {
            quotient += 1;
            remainder = divisor - remainder;
            !self.negative()
        } else {
            self.negative()
        };
        let result = if remainder == 0 {
            Rounded::exact(
                Self::from_parts(self.negative(), 0, 0),
                self.operand_exceptions() | rhs.operand_exceptions(),
            )
        } else {
            let high = 127 - remainder.leading_zeros();
            let extended = if high > 66 {
                shift_jam(remainder, high - 66)
            } else {
                remainder << (66 - high)
            };
            round(
                negative,
                base + high as i32 - 63,
                extended,
                0x037f,
                self.operand_exceptions() | rhs.operand_exceptions(),
            )
        };
        let signed_quotient = if self.negative() ^ rhs.negative() {
            (quotient as u64).wrapping_neg()
        } else {
            quotient as u64
        };
        (result, partial, (signed_quotient & 7) as u8)
    }

    pub fn from_f32_bits(bits: u32) -> Self {
        Self::from_ieee(u64::from(bits), 8, 23)
    }
    pub fn from_f64_bits(bits: u64) -> Self {
        Self::from_ieee(bits, 11, 52)
    }

    fn from_ieee(bits: u64, exponent_bits: u32, fraction_bits: u32) -> Self {
        let negative = bits >> (exponent_bits + fraction_bits) != 0;
        let exponent_mask = (1_u64 << exponent_bits) - 1;
        let exponent = (bits >> fraction_bits) & exponent_mask;
        let fraction = bits & ((1_u64 << fraction_bits) - 1);
        if exponent == exponent_mask {
            return Self::from_parts(
                negative,
                0x7fff,
                (1 << 63) | (fraction << (63 - fraction_bits)),
            );
        }
        if exponent == 0 && fraction == 0 {
            return Self::from_parts(negative, 0, 0);
        }
        let significand = fraction | if exponent != 0 { 1 << fraction_bits } else { 0 };
        let shift = significand.leading_zeros();
        let unbiased = exponent.max(1) as i32 - ((1 << (exponent_bits - 1)) - 1) + 63
            - shift as i32
            - fraction_bits as i32;
        Self::from_parts(negative, (unbiased + 16383) as u16, significand << shift)
    }

    pub(crate) fn to_ieee(
        self,
        exponent_bits: u32,
        fraction_bits: u32,
        mode: X87Rounding,
    ) -> (u64, u16, bool) {
        let sign = u64::from(self.negative()) << (exponent_bits + fraction_bits);
        let max_exponent = (1_u64 << exponent_bits) - 1;
        let bias = (1 << (exponent_bits - 1)) - 1;
        match self.kind() {
            Kind::Zero => (sign, 0, false),
            Kind::Infinity => (sign | (max_exponent << fraction_bits), 0, false),
            Kind::Unsupported => (
                sign | (max_exponent << fraction_bits) | (1 << (fraction_bits - 1)),
                INVALID,
                false,
            ),
            Kind::NaN { signaling } => (
                sign | (max_exponent << fraction_bits)
                    | (self.significand() >> (63 - fraction_bits))
                    | (1 << (fraction_bits - 1)),
                if signaling { INVALID } else { 0 },
                false,
            ),
            Kind::Finite {
                significand,
                exponent,
            } => {
                let min = 1 - bias;
                let mut exponent = exponent;
                let mut extended = u128::from(significand) << 3;
                let (unbounded, _, _) =
                    rounded_significand(extended, 66 - fraction_bits, self.negative(), mode);
                let tiny = exponent + i32::from(unbounded >= 1_u128 << (fraction_bits + 1)) < min;
                if exponent < min {
                    extended = shift_jam(extended, (min - exponent) as u32);
                    exponent = min;
                }
                let (mut significand, inexact, incremented) =
                    rounded_significand(extended, 66 - fraction_bits, self.negative(), mode);
                if significand >= 1_u128 << (fraction_bits + 1) {
                    significand >>= 1;
                    exponent += 1;
                }
                let mut exceptions =
                    self.operand_exceptions() | if inexact { PRECISION } else { 0 };
                if exponent > bias {
                    exceptions |= OVERFLOW | PRECISION;
                    let infinity = mode == X87Rounding::Nearest
                        || (mode == X87Rounding::Up && !self.negative())
                        || (mode == X87Rounding::Down && self.negative());
                    let bits = if infinity {
                        max_exponent << fraction_bits
                    } else {
                        ((max_exponent - 1) << fraction_bits) | ((1 << fraction_bits) - 1)
                    };
                    return (sign | bits, exceptions, infinity);
                }
                let normal = significand >= 1_u128 << fraction_bits;
                if tiny && inexact {
                    exceptions |= UNDERFLOW;
                }
                let encoded_exponent = if normal { (exponent + bias) as u64 } else { 0 };
                (
                    sign | (encoded_exponent << fraction_bits)
                        | (significand as u64 & ((1 << fraction_bits) - 1)),
                    exceptions,
                    incremented,
                )
            }
        }
    }

    pub(crate) fn to_ieee_with_control(
        self,
        exponent_bits: u32,
        fraction_bits: u32,
        control: u16,
    ) -> (u64, u16, bool) {
        let mode = X87Rounding::from_control(control);
        let (bits, mut exceptions, incremented) = self.to_ieee(exponent_bits, fraction_bits, mode);
        if control & UNDERFLOW == 0
            && let Kind::Finite {
                significand,
                exponent,
            } = self.kind()
        {
            let (rounded, _, _) = rounded_significand(
                u128::from(significand) << 3,
                66 - fraction_bits,
                self.negative(),
                mode,
            );
            let exponent = exponent + i32::from(rounded >= 1_u128 << (fraction_bits + 1));
            if exponent < 2 - (1_i32 << (exponent_bits - 1)) {
                exceptions |= UNDERFLOW;
            }
        }
        (bits, exceptions, incremented)
    }

    pub(crate) fn to_integer(self, bits: u32, mode: X87Rounding) -> (u64, u16, bool) {
        let indefinite = 1_u64 << (bits - 1);
        match self.kind() {
            Kind::Zero => (0, 0, false),
            Kind::Finite {
                significand,
                exponent,
            } => {
                if exponent > 63 {
                    return (indefinite, INVALID, false);
                }
                let shift = 63 - exponent;
                let extended = u128::from(significand) << 3;
                let (magnitude, inexact, incremented) = if shift >= 0 {
                    rounded_significand(extended, (shift + 3) as u32, self.negative(), mode)
                } else {
                    (extended << (-shift as u32) >> 3, false, false)
                };
                let limit = 1_u128 << (bits - 1);
                if magnitude > limit || (!self.negative() && magnitude == limit) {
                    return (indefinite, INVALID, false);
                }
                let result = if self.negative() {
                    (magnitude as u64).wrapping_neg()
                } else {
                    magnitude as u64
                };
                (
                    result,
                    self.operand_exceptions() | if inexact { PRECISION } else { 0 },
                    incremented,
                )
            }
            _ => (indefinite, INVALID, false),
        }
    }

    pub(crate) fn round_integer(self, control: u16) -> Rounded {
        if let Some(result) = Self::special(self, Self::ZERO) {
            return result;
        }
        let Kind::Finite {
            significand,
            exponent,
        } = self.kind()
        else {
            return Rounded::exact(self, 0);
        };
        if exponent >= 63 {
            return Rounded::exact(self, self.operand_exceptions());
        }
        let (magnitude, inexact, incremented) = rounded_significand(
            u128::from(significand) << 3,
            (66 - exponent) as u32,
            self.negative(),
            X87Rounding::from_control(control),
        );
        let value = if magnitude == 0 {
            Self::from_parts(self.negative(), 0, 0)
        } else {
            let shift = (magnitude as u64).leading_zeros();
            Self::from_parts(
                self.negative(),
                (16383 + 63 - shift) as u16,
                (magnitude as u64) << shift,
            )
        };
        Rounded {
            value,
            exceptions: self.operand_exceptions() | if inexact { PRECISION } else { 0 },
            incremented,
        }
    }

    pub(crate) fn scaled(self, scale: Self, control: u16) -> Rounded {
        if let Some(result) = Self::special(self, scale) {
            return result;
        }
        let exceptions = self.operand_exceptions() | scale.operand_exceptions();
        if matches!(scale.kind(), Kind::Infinity) {
            return match (self.kind(), scale.negative()) {
                (Kind::Infinity, true) | (Kind::Zero, false) => Rounded::invalid(),
                (Kind::Zero | Kind::Infinity, _) => Rounded::exact(self, exceptions),
                (Kind::Finite { .. }, true) => {
                    Rounded::exact(Self::from_parts(self.negative(), 0, 0), exceptions)
                }
                (Kind::Finite { .. }, false) => Rounded::exact(
                    Self::from_parts(self.negative(), 0x7fff, 1 << 63),
                    exceptions,
                ),
                _ => Rounded::invalid(),
            };
        }
        let Kind::Finite {
            significand,
            exponent,
        } = self.kind()
        else {
            return Rounded::exact(self, exceptions);
        };
        let (integer, flags, _) = scale.to_integer(32, X87Rounding::Zero);
        let delta = if flags & INVALID != 0 {
            if scale.negative() { -32768 } else { 32768 }
        } else {
            (integer as i32).clamp(-32768, 32768)
        };
        round(
            self.negative(),
            exponent + delta,
            u128::from(significand) << 3,
            control | 0x0300,
            exceptions,
        )
    }
}

impl Rounded {
    pub(crate) fn exact(value: Ext80, exceptions: u16) -> Self {
        Self {
            value,
            exceptions,
            incremented: false,
        }
    }
    pub(crate) fn invalid() -> Self {
        Self::exact(Ext80::INDEFINITE, INVALID)
    }
}

pub(super) fn round(
    negative: bool,
    mut exponent: i32,
    mut extended: u128,
    control: u16,
    mut exceptions: u16,
) -> Rounded {
    if extended == 0 {
        return Rounded::exact(Ext80::from_parts(negative, 0, 0), exceptions);
    }
    let high = 127 - extended.leading_zeros();
    if high > 66 {
        extended = shift_jam(extended, high - 66);
        exponent += (high - 66) as i32;
    }
    if high < 66 {
        extended <<= 66 - high;
        exponent -= (66 - high) as i32;
    }
    let precision = match (control >> 8) & 3 {
        0 => 24,
        2 => 53,
        _ => 64,
    };
    let mode = X87Rounding::from_control(control);
    let (mut unbounded, unbounded_inexact, unbounded_incremented) =
        rounded_significand(extended, 67 - precision, negative, mode);
    let mut unbounded_exponent = exponent;
    if unbounded >= 1_u128 << precision {
        unbounded >>= 1;
        unbounded_exponent += 1;
    }
    let range_exception = if unbounded_exponent > 16383 {
        OVERFLOW
    } else if unbounded_exponent < -16382 {
        UNDERFLOW
    } else {
        0
    };
    if range_exception != 0 && control & range_exception == 0 {
        // unmasked register range results retain a rounded significand and a biased exponent.
        let biased = unbounded_exponent
            + if range_exception == OVERFLOW {
                -24576
            } else {
                24576
            };
        let value = if biased > 16383 {
            Ext80::from_parts(negative, 0x7fff, 1 << 63)
        } else if biased < -16382 {
            Ext80::from_parts(negative, 0, 0)
        } else {
            Ext80::from_parts(
                negative,
                (biased + 16383) as u16,
                (unbounded << (64 - precision)) as u64,
            )
        };
        return Rounded {
            value,
            exceptions: exceptions
                | range_exception
                | if unbounded_inexact { PRECISION } else { 0 },
            incremented: unbounded_incremented,
        };
    }
    if exponent < -16382 {
        extended = shift_jam(extended, (-16382 - exponent) as u32);
        exponent = -16382;
    }
    let (mut significand, inexact, incremented) =
        rounded_significand(extended, 67 - precision, negative, mode);
    if significand >= 1_u128 << precision {
        significand >>= 1;
        exponent += 1;
    }
    significand <<= 64 - precision;
    if inexact {
        exceptions |= PRECISION;
    }
    if exponent > 16383 {
        exceptions |= OVERFLOW | PRECISION;
        let infinity = mode == X87Rounding::Nearest
            || (mode == X87Rounding::Up && !negative)
            || (mode == X87Rounding::Down && negative);
        return Rounded {
            value: Ext80::from_parts(
                negative,
                if infinity { 0x7fff } else { 0x7ffe },
                if infinity {
                    1 << 63
                } else {
                    u64::MAX << (64 - precision)
                },
            ),
            exceptions,
            incremented: infinity,
        };
    }
    let normal = significand >= 1 << 63;
    if range_exception == UNDERFLOW && inexact {
        exceptions |= UNDERFLOW;
    }
    Rounded {
        value: Ext80::from_parts(
            negative,
            if normal { (exponent + 16383) as u16 } else { 0 },
            significand as u64,
        ),
        exceptions,
        incremented,
    }
}

fn rounded_significand(
    value: u128,
    shift: u32,
    negative: bool,
    mode: X87Rounding,
) -> (u128, bool, bool) {
    if shift == 0 {
        return (value, false, false);
    }
    let retained = if shift >= 128 { 0 } else { value >> shift };
    let discarded = if shift >= 128 {
        value
    } else {
        value & ((1_u128 << shift) - 1)
    };
    let inexact = discarded != 0;
    let half = if shift > 128 {
        None
    } else {
        Some(1_u128 << (shift - 1))
    };
    let increment = match mode {
        X87Rounding::Nearest => {
            half.is_some_and(|half| discarded > half || (discarded == half && retained & 1 != 0))
        }
        X87Rounding::Down => negative && inexact,
        X87Rounding::Up => !negative && inexact,
        X87Rounding::Zero => false,
    };
    (retained + u128::from(increment), inexact, increment)
}

fn shift_jam(value: u128, shift: u32) -> u128 {
    if shift == 0 {
        value
    } else if shift >= 128 {
        u128::from(value != 0)
    } else {
        (value >> shift) | u128::from(value & ((1_u128 << shift) - 1) != 0)
    }
}

fn integer_sqrt(value: u128) -> u128 {
    let mut remainder = value;
    let mut result = 0;
    let mut bit = 1_u128 << 126;
    while bit > remainder {
        bit >>= 2;
    }
    while bit != 0 {
        if remainder >= result + bit {
            remainder -= result + bit;
            result = (result >> 1) + bit;
        } else {
            result >>= 1;
        }
        bit >>= 2;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_apfloat::{Float, Round, ieee::X87DoubleExtended};

    fn bits(value: Ext80) -> u128 {
        u128::from(value.significand())
            | (u128::from(u16::from_le_bytes([value.0[8], value.0[9]])) << 64)
    }

    #[test]
    fn finite_full_precision_arithmetic_matches_independent_apfloat() {
        let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
        for _ in 0..256 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let a = Ext80::from_parts(
                seed & 1 != 0,
                (16383 + (seed % 401) as i32 - 200) as u16,
                seed | (1 << 63),
            );
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let b = Ext80::from_parts(
                seed & 1 != 0,
                (16383 + (seed % 401) as i32 - 200) as u16,
                seed | (1 << 63),
            );
            for (control, round) in [
                (0x037f, Round::NearestTiesToEven),
                (0x077f, Round::TowardNegative),
                (0x0b7f, Round::TowardPositive),
                (0x0f7f, Round::TowardZero),
            ] {
                let av = X87DoubleExtended::from_bits(bits(a));
                let bv = X87DoubleExtended::from_bits(bits(b));
                for (name, actual, expected) in [
                    ("add", a.add(b, control), av.add_r(bv, round)),
                    ("multiply", a.multiply(b, control), av.mul_r(bv, round)),
                    ("divide", a.divide(b, control), av.div_r(bv, round)),
                ] {
                    assert_eq!(
                        bits(actual.value),
                        expected.value.to_bits(),
                        "{name} a={a:?} b={b:?} control={control:x}"
                    );
                }
            }
        }
    }

    #[test]
    fn extended_add_keeps_the_bit_beyond_binary64_and_honors_precision_control() {
        let tiny = Ext80::from_parts(false, 16383 - 63, 1 << 63);
        let extended = Ext80::ONE.add(tiny, 0x037f);
        assert_eq!(
            extended.value,
            Ext80::from_parts(false, 16383, (1 << 63) | 1)
        );
        assert_eq!(extended.exceptions, 0);
        let double = Ext80::ONE.add(tiny, 0x027f);
        assert_eq!(double.value, Ext80::ONE);
        assert_eq!(double.exceptions, PRECISION);
        let upper = Ext80::ONE.add(tiny, 0x0a7f);
        assert_eq!(
            upper.value,
            Ext80::from_parts(false, 16383, (1 << 63) | (1 << 11))
        );
        let single = Ext80::ONE.add(tiny, 0x087f);
        assert_eq!(
            single.value,
            Ext80::from_parts(false, 16383, (1 << 63) | (1 << 40))
        );
    }

    #[test]
    fn multiply_divide_sqrt_round_exact_extended_significands() {
        let two = Ext80::from_i64(2);
        assert_eq!(two.multiply(two, 0x037f).value, Ext80::from_i64(4));
        let third = Ext80::ONE.divide(Ext80::from_i64(3), 0x037f);
        assert_eq!(
            third.value,
            Ext80::from_parts(false, 16381, 0xaaaa_aaaa_aaaa_aaab)
        );
        assert_eq!(third.exceptions, PRECISION);
        assert_eq!(
            two.sqrt(0x037f).value,
            Ext80::from_parts(false, 16383, 0xb504_f333_f9de_6484)
        );
        assert_eq!(Ext80::from_i64(4).sqrt(0x037f).value, two);
    }

    #[test]
    fn ieee_conversion_is_bit_exact_and_uses_requested_rounding() {
        for bits in [
            0,
            0x8000_0000,
            0x3f80_0000,
            0x3fc0_0000,
            1,
            0x007f_ffff,
            0x7f7f_ffff,
            0x7f80_0000,
        ] {
            let value = Ext80::from_f32_bits(bits);
            assert_eq!(
                value.to_ieee(8, 23, X87Rounding::Nearest).0,
                u64::from(bits)
            );
        }
        for bits in [
            0,
            0x8000_0000_0000_0000,
            0x3ff0_0000_0000_0000,
            1,
            0x000f_ffff_ffff_ffff,
            0x7fef_ffff_ffff_ffff,
        ] {
            assert_eq!(
                Ext80::from_f64_bits(bits)
                    .to_ieee(11, 52, X87Rounding::Nearest)
                    .0,
                bits
            );
        }
        let half = Ext80::from_parts(false, 16383, 0xc000_0000_0000_0000);
        assert_eq!(
            half.to_integer(32, X87Rounding::Nearest),
            (2, PRECISION, true)
        );
        assert_eq!(
            half.to_integer(32, X87Rounding::Zero),
            (1, PRECISION, false)
        );
        assert_eq!(
            half.negated().to_integer(32, X87Rounding::Down).0,
            (-2_i64) as u64
        );
        assert_eq!(
            Ext80::from_i64(i64::MIN)
                .to_integer(64, X87Rounding::Nearest)
                .0,
            i64::MIN as u64
        );
    }

    #[test]
    fn denormals_specials_and_remainder_do_not_use_host_float() {
        let sub = Ext80::from_parts(false, 0, 1);
        assert_eq!(sub.add(sub, 0x037f).value, Ext80::from_parts(false, 0, 2));
        assert_eq!(sub.add(sub, 0x037f).exceptions, DENORMAL);
        assert_eq!(
            Ext80::ONE.divide(Ext80::ZERO, 0x037f).exceptions,
            ZERO_DIVIDE
        );
        assert_eq!(
            Ext80::ZERO.divide(Ext80::ZERO, 0x037f).value,
            Ext80::INDEFINITE
        );
        let (rem, partial, q) = Ext80::from_i64(7).remainder(Ext80::from_i64(2), false);
        assert!(!partial);
        assert_eq!(rem.value, Ext80::ONE);
        assert_eq!(q, 3);
        let (rem, partial, q) = Ext80::from_i64(7).remainder(Ext80::from_i64(2), true);
        assert!(!partial);
        assert_eq!(rem.value, Ext80::ONE.negated());
        assert_eq!(q, 4);
    }
}
