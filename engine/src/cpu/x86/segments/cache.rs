use super::{SegmentRegister, SegmentState};

/// loaded code/data descriptor attributes, separate from the mutable descriptor table.
/// the descriptor S bit is implied 1; L is 0 in the fixed 32-bit execution profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentCache(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentAccess {
    Read,
    Write,
    Execute,
    /// CLFLUSH accepts execute-only code segments as well as data segments.
    CacheFlush,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentAccessError {
    Unusable,
    Permission,
    Limit,
}

impl SegmentCache {
    pub const UNUSABLE: Self = Self(0);
    const USABLE: u32 = 1 << 28;
    const PRESENT: u32 = 1 << 31;
    const LONG: u32 = 1 << 26;

    pub const fn from_bits(bits: u32) -> Option<Self> {
        if bits == 0
            || (bits & Self::USABLE != 0 && bits & Self::PRESENT != 0 && bits & Self::LONG == 0)
        {
            Some(Self(bits))
        } else {
            None
        }
    }

    pub const fn bits(self) -> u32 {
        self.0
    }
    pub const fn usable(self) -> bool {
        self.0 & Self::USABLE != 0
    }
    pub const fn code(self) -> bool {
        self.0 & (8 << 20) != 0
    }
    pub const fn conforming(self) -> bool {
        self.code() && self.0 & (4 << 20) != 0
    }
    pub const fn readable(self) -> bool {
        !self.code() || self.0 & (2 << 20) != 0
    }
    pub const fn writable(self) -> bool {
        !self.code() && self.0 & (2 << 20) != 0
    }
    pub const fn default32(self) -> bool {
        self.0 & (1 << 25) != 0
    }
    pub const fn dpl(self) -> u8 {
        ((self.0 >> 29) & 3) as u8
    }

    pub const fn limit(self) -> u32 {
        let raw = self.0 & 0x000f_ffff;
        if self.0 & (1 << 24) != 0 {
            (raw << 12) | 0xfff
        } else {
            raw
        }
    }

    pub(super) const fn flat(kind: u8, dpl: u8) -> Self {
        // loaded descriptors carry the accessed bit, a four-gibibyte limit and D/B=1.
        Self(
            0x000f_ffff
                | ((kind as u32 | 1) << 20)
                | (1 << 24)
                | (1 << 25)
                | Self::USABLE
                | ((dpl as u32) << 29)
                | Self::PRESENT,
        )
    }

    pub fn check_span(
        self,
        offset: u32,
        width: u32,
        access: SegmentAccess,
    ) -> Result<(), SegmentAccessError> {
        if !self.usable() {
            return Err(SegmentAccessError::Unusable);
        }
        let permitted = match access {
            SegmentAccess::Read => self.readable(),
            SegmentAccess::Write => self.writable(),
            SegmentAccess::Execute => self.code(),
            SegmentAccess::CacheFlush => true,
        };
        if !permitted {
            return Err(SegmentAccessError::Permission);
        }
        let end = width
            .checked_sub(1)
            .and_then(|last| offset.checked_add(last))
            .ok_or(SegmentAccessError::Limit)?;
        let expand_down = !self.code() && self.0 & (4 << 20) != 0;
        let in_range = if expand_down {
            offset > self.limit()
                && end
                    <= if self.default32() {
                        u32::MAX
                    } else {
                        u16::MAX.into()
                    }
        } else {
            end <= self.limit()
        };
        if in_range {
            Ok(())
        } else {
            Err(SegmentAccessError::Limit)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentContext {
    pub state: SegmentState,
    pub caches: [SegmentCache; 6],
}

impl Default for SegmentContext {
    fn default() -> Self {
        Self::legacy(SegmentState::default())
    }
}

impl SegmentContext {
    /// pure conversion from the legacy flat execution profile, without table lookup.
    pub fn legacy(state: SegmentState) -> Self {
        Self {
            state,
            caches: std::array::from_fn(|index| {
                if state.selectors[index] & !3 == 0 {
                    SegmentCache::UNUSABLE
                } else {
                    SegmentCache::flat(
                        if index == SegmentRegister::Cs.index() {
                            0xb
                        } else {
                            3
                        },
                        3,
                    )
                }
            }),
        }
    }

    pub fn is_legacy(&self) -> bool {
        self.state.bases[..4].iter().all(|base| *base == 0)
            && self.caches == Self::legacy(self.state).caches
    }

    pub fn is_valid(&self) -> bool {
        if !self.state.is_valid() {
            return false;
        }
        self.caches
            .iter()
            .copied()
            .enumerate()
            .all(|(index, cache)| {
                let selector = self.state.selectors[index];
                if selector & !3 == 0 {
                    return cache == SegmentCache::UNUSABLE;
                }
                if !cache.usable() {
                    return false;
                }
                match index {
                    1 => {
                        cache.code()
                            && cache.default32()
                            && (cache.conforming() || cache.dpl() == 3)
                    }
                    2 => cache.writable() && cache.default32() && cache.dpl() == 3,
                    _ => cache.readable() && (cache.conforming() || cache.dpl() == 3),
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache(kind: u8, limit: u32, granularity: bool, default32: bool) -> SegmentCache {
        SegmentCache::from_bits(
            limit
                | (u32::from(kind) << 20)
                | (u32::from(granularity) << 24)
                | (u32::from(default32) << 25)
                | 0xf000_0000,
        )
        .unwrap()
    }

    #[test]
    fn normal_and_expand_down_spans_check_the_entire_width_without_wrapping() {
        let normal = cache(3, 0xfff, false, true);
        assert_eq!(normal.check_span(0xffe, 2, SegmentAccess::Write), Ok(()));
        assert_eq!(
            normal.check_span(0xfff, 2, SegmentAccess::Write),
            Err(SegmentAccessError::Limit)
        );
        assert_eq!(
            normal.check_span(0, 0, SegmentAccess::Read),
            Err(SegmentAccessError::Limit)
        );
        let pages = cache(3, 1, true, true);
        assert_eq!(pages.limit(), 0x1fff);
        for default32 in [false, true] {
            let down = cache(7, 0xfff, false, default32);
            assert_eq!(
                down.check_span(0xfff, 1, SegmentAccess::Read),
                Err(SegmentAccessError::Limit)
            );
            assert_eq!(down.check_span(0x1000, 2, SegmentAccess::Read), Ok(()));
            assert_eq!(
                down.check_span(0xffff, 2, SegmentAccess::Read),
                if default32 {
                    Ok(())
                } else {
                    Err(SegmentAccessError::Limit)
                }
            );
            assert_eq!(
                down.check_span(u32::MAX, 2, SegmentAccess::Read),
                Err(SegmentAccessError::Limit)
            );
        }
    }

    #[test]
    fn access_type_checks_are_separate_from_range_checks() {
        let readonly = cache(1, 0xfff, false, true);
        assert_eq!(readonly.check_span(0, 1, SegmentAccess::Read), Ok(()));
        assert_eq!(
            readonly.check_span(0, 1, SegmentAccess::Write),
            Err(SegmentAccessError::Permission)
        );
        assert_eq!(
            readonly.check_span(0, 1, SegmentAccess::Execute),
            Err(SegmentAccessError::Permission)
        );
        let execute_only = cache(9, 0xfff, false, true);
        assert_eq!(
            execute_only.check_span(0, 1, SegmentAccess::Execute),
            Ok(())
        );
        assert_eq!(
            execute_only.check_span(0, 1, SegmentAccess::Read),
            Err(SegmentAccessError::Permission)
        );
        assert_eq!(
            execute_only.check_span(0, 1, SegmentAccess::CacheFlush),
            Ok(())
        );
        assert_eq!(
            SegmentCache::UNUSABLE.check_span(0, 1, SegmentAccess::Read),
            Err(SegmentAccessError::Unusable)
        );
    }

    #[test]
    fn loaded_context_rejects_incoherent_mode_type_privilege_and_usability() {
        let valid = SegmentContext::default();
        assert!(valid.is_valid() && valid.is_legacy());
        for (index, bits) in [
            (1, 0xf1bf_ffff),
            (2, 0xf13f_ffff),
            (1, 0xf33f_ffff),
            (2, 0xf31f_ffff),
            (3, 0xf39f_ffff),
            (3, 0x933f_ffff),
            (4, 0),
        ] {
            let mut context = valid;
            context.caches[index] = SegmentCache::from_bits(bits).unwrap();
            assert!(!context.is_valid(), "index={index}, bits={bits:x}");
        }
        for bits in [1, 1 << 28, 1 << 31, 0xf73f_ffff] {
            assert!(SegmentCache::from_bits(bits).is_none());
        }
        let mut null = valid;
        null.state.selectors[4] = 0;
        assert!(!null.is_valid());
        null.caches[4] = SegmentCache::UNUSABLE;
        assert!(null.is_valid() && null.is_legacy());
        let mut conforming = valid;
        conforming.caches[1] = cache(0xf, 0xfffff, true, true);
        conforming.caches[4] = SegmentCache::from_bits(0x93ff_ffff).unwrap();
        assert!(conforming.is_valid() && !conforming.is_legacy());
    }
    #[test]
    fn current_context_accepts_data_and_stack_bases_but_keeps_d32_b32_profile() {
        let mut context = SegmentContext::default();
        for segment in [
            SegmentRegister::Es,
            SegmentRegister::Ss,
            SegmentRegister::Ds,
            SegmentRegister::Fs,
            SegmentRegister::Gs,
        ] {
            assert!(context.state.set_base(segment, 0x1234_0000));
        }
        assert!(context.is_valid());
        assert!(!context.is_legacy());
        assert!(!context.state.set_base(SegmentRegister::Cs, 1));
        let mut nonflat_code = context;
        nonflat_code.state.bases[SegmentRegister::Cs.index()] = 1;
        assert!(!nonflat_code.is_valid());
        for segment in [SegmentRegister::Cs, SegmentRegister::Ss] {
            let mut invalid = context;
            invalid.caches[segment.index()] = cache(
                if segment == SegmentRegister::Cs {
                    0xb
                } else {
                    3
                },
                0xfffff,
                true,
                false,
            );
            assert!(!invalid.is_valid());
        }
    }
}
