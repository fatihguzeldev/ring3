use super::{
    Register32,
    ir::{AddressSize, EffectiveAddress},
};

pub const USER_POP_FLAGS_MASK: u32 = 0x0024_4dd5;
pub const USER_POP_FLAGS_WORD_MASK: u32 = 0x4dd5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlOperation {
    PopFlags {
        word: bool,
    },
    InvalidOpcode,
    GeneralProtectionZero,
    /// denied port strings skip the attempt only for a zero selected repeat count.
    DeniedPortString {
        repeat_count: Option<AddressSize>,
    },
    Breakpoint,
    Overflow,
    Bound {
        index: Register32,
        address: EffectiveAddress,
        word: bool,
    },
}

impl ControlOperation {
    pub const fn reads_memory(self) -> bool {
        matches!(self, Self::PopFlags { .. } | Self::Bound { .. })
    }
}

/// apply cpl3/iopl0 writes; privileged bits are preserved and the resume flag is cleared.
pub const fn pop_flags(current: u32, popped: u32, word: bool) -> u32 {
    let mutable = if word {
        USER_POP_FLAGS_WORD_MASK
    } else {
        USER_POP_FLAGS_MASK
    };
    (current & !mutable & !0x10000) | (popped & mutable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popfd_ignores_privileged_reserved_and_resume_bits() {
        assert_eq!(pop_flags(2, u32::MAX, false), 0x244dd7);
        assert_eq!(pop_flags(0x3202, 0, false), 0x3202);
        assert_eq!(pop_flags(0x10002, u32::MAX, false) & 0x10000, 0);
    }

    #[test]
    fn word_pop_changes_low_user_flags_without_ac_or_id() {
        assert_eq!(pop_flags(0x240002, 0xffff, true), 0x244dd7);
        assert_eq!(pop_flags(0x240002, 0, true), 0x240002);
    }
}
