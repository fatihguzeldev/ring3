/// integer read/modify/write semantics shared by the checked private-memory transaction.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtomicKind {
    Add,
    AddCarry,
    Subtract,
    SubtractBorrow,
    And,
    Or,
    Xor,
    Increment,
    Decrement,
    Negate,
    Not,
    BitSet,
    BitReset,
    BitComplement,
}

impl AtomicKind {
    pub fn decode(value: u32) -> Option<Self> {
        Some(match value {
            0 => Self::Add,
            1 => Self::AddCarry,
            2 => Self::Subtract,
            3 => Self::SubtractBorrow,
            4 => Self::And,
            5 => Self::Or,
            6 => Self::Xor,
            7 => Self::Increment,
            8 => Self::Decrement,
            9 => Self::Negate,
            10 => Self::Not,
            11 => Self::BitSet,
            12 => Self::BitReset,
            13 => Self::BitComplement,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtomicResult {
    pub value: u32,
    pub flags: u32,
}

pub fn evaluate(
    kind: AtomicKind,
    width: u32,
    old: u32,
    source: u32,
    flags: u32,
) -> Option<AtomicResult> {
    if !matches!(width, 1 | 2 | 4) {
        return None;
    }
    let bits = width * 8;
    let mask = u32::MAX >> (32 - bits);
    let sign = 1 << (bits - 1);
    let old = old & mask;
    let source = source & mask;
    let carry = flags & 1;
    let (value, lhs, rhs, subtract, carry_out) = match kind {
        AtomicKind::Add | AtomicKind::AddCarry => {
            let c = if kind == AtomicKind::AddCarry {
                carry
            } else {
                0
            };
            let sum = u64::from(old) + u64::from(source) + u64::from(c);
            (sum as u32 & mask, old, source, false, sum > u64::from(mask))
        }
        AtomicKind::Subtract | AtomicKind::SubtractBorrow => {
            let c = if kind == AtomicKind::SubtractBorrow {
                carry
            } else {
                0
            };
            (
                old.wrapping_sub(source).wrapping_sub(c) & mask,
                old,
                source,
                true,
                u64::from(old) < u64::from(source) + u64::from(c),
            )
        }
        AtomicKind::Increment => (old.wrapping_add(1) & mask, old, 1, false, carry != 0),
        AtomicKind::Decrement => (old.wrapping_sub(1) & mask, old, 1, true, carry != 0),
        AtomicKind::Negate => (old.wrapping_neg() & mask, 0, old, true, old != 0),
        AtomicKind::Not => {
            return Some(AtomicResult {
                value: !old & mask,
                flags,
            });
        }
        AtomicKind::BitSet | AtomicKind::BitReset | AtomicKind::BitComplement => {
            let bit = 1 << (source & (bits - 1));
            return Some(AtomicResult {
                value: match kind {
                    AtomicKind::BitSet => old | bit,
                    AtomicKind::BitReset => old & !bit,
                    _ => old ^ bit,
                },
                flags: (flags & !1) | u32::from(old & bit != 0),
            });
        }
        AtomicKind::And | AtomicKind::Or | AtomicKind::Xor => {
            let value = match kind {
                AtomicKind::And => old & source,
                AtomicKind::Or => old | source,
                _ => old ^ source,
            };
            return Some(AtomicResult {
                value,
                flags: status(flags, value, sign),
            });
        }
    };
    let overflow = if subtract {
        (lhs ^ rhs) & (lhs ^ value) & sign != 0
    } else {
        !(lhs ^ rhs) & (lhs ^ value) & sign != 0
    };
    Some(AtomicResult {
        value,
        flags: status(flags, value, sign)
            | u32::from(carry_out)
            | ((lhs ^ rhs ^ value) & 0x10)
            | (u32::from(overflow) << 11),
    })
}

fn status(flags: u32, value: u32, sign: u32) -> u32 {
    // AF is undefined for logical instructions; this profile deterministically clears it.
    (flags & !0x8d5)
        | 2
        | (u32::from((value as u8).count_ones() & 1 == 0) << 2)
        | (u32::from(value == 0) << 6)
        | (u32::from(value & sign != 0) << 7)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn carry_borrow_overflow_and_preserved_flags_have_distinct_boundaries() {
        for width in [1, 2, 4] {
            let mask = u32::MAX >> (32 - width * 8);
            let sign = 1 << (width * 8 - 1);
            assert_eq!(
                evaluate(AtomicKind::AddCarry, width, mask, 0, 0x403).unwrap(),
                AtomicResult {
                    value: 0,
                    flags: 0x457
                }
            );
            assert_eq!(
                evaluate(AtomicKind::SubtractBorrow, width, 0, mask, 3).unwrap(),
                AtomicResult {
                    value: 0,
                    flags: 0x57
                }
            );
            let inc = evaluate(AtomicKind::Increment, width, sign - 1, 0, 0x403).unwrap();
            assert_eq!(inc.value, sign);
            assert_eq!(inc.flags & 0xc81, 0xc81);
            let neg = evaluate(AtomicKind::Negate, width, sign, 0, 2).unwrap();
            assert_eq!(neg.value, sign);
            assert_eq!(neg.flags & 0x801, 0x801);
            assert_eq!(
                evaluate(AtomicKind::Not, width, 0, 0, 0xcd7).unwrap().flags,
                0xcd7
            );
        }
    }
    #[test]
    fn bit_operations_change_only_cf_and_use_width_modulo() {
        for (kind, value) in [
            (AtomicKind::BitSet, 0x8001),
            (AtomicKind::BitReset, 0x8000),
            (AtomicKind::BitComplement, 0x8000),
        ] {
            assert_eq!(
                evaluate(kind, 2, 0x8001, 16, 0xcd6).unwrap(),
                AtomicResult {
                    value,
                    flags: 0xcd7
                }
            );
        }
        assert!(evaluate(AtomicKind::Add, 8, 0, 0, 2).is_none());
    }
}
