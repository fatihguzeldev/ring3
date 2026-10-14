use super::{
    Register32,
    ir::{AddressSize, EffectiveAddress},
};

pub const USER_POP_FLAGS_MASK: u32 = 0x0024_4dd5;
pub const USER_POP_FLAGS_WORD_MASK: u32 = 0x4dd5;
pub const USER_IRET_FLAGS_MASK: u32 = 0x0025_4dd5;
pub const USER_IRET_FLAGS_WORD_MASK: u32 = 0x4dd5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlOperation {
    PopFlags {
        word: bool,
    },
    InvalidOpcode,
    DeviceNotAvailable,
    GeneralProtectionZero,
    /// denied port strings skip the attempt only for a zero selected repeat count.
    DeniedPortString {
        repeat_count: Option<AddressSize>,
    },
    Breakpoint,
    DebugTrap,
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

/// restore the successful cpl3 flag image after instruction-start rf consumption.
pub const fn interrupt_return_flags(current: u32, popped: u32, word: bool) -> u32 {
    let mutable = if word {
        USER_IRET_FLAGS_WORD_MASK
    } else {
        USER_IRET_FLAGS_MASK
    };
    (current & !mutable) | (popped & mutable)
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

    #[test]
    fn interrupt_return_mutable_and_preserved_bits_match_independent_lists() {
        let common = [0, 2, 4, 6, 7, 8, 10, 11, 14];
        let dword_only = [16, 18, 21];
        for word in [false, true] {
            for bit in 0..32 {
                let mutable = common.contains(&bit) || (!word && dword_only.contains(&bit));
                let flag = 1_u32 << bit;
                let current = 2 | flag;
                assert_eq!(
                    interrupt_return_flags(current, 0, word),
                    if mutable { 2 } else { current },
                    "clear word={word} bit={bit}"
                );
                assert_eq!(
                    interrupt_return_flags(2, flag, word),
                    if mutable { 2 | flag } else { 2 },
                    "set word={word} bit={bit}"
                );
            }
        }
    }

    #[test]
    fn interrupt_return_ignores_frame_vm_and_privileged_bits_and_can_load_nt() {
        assert_eq!(interrupt_return_flags(2, u32::MAX, false), 0x254dd7);
        assert_eq!(interrupt_return_flags(2, u32::MAX, true), 0x4dd7);
        for word in [false, true] {
            assert_eq!(interrupt_return_flags(0x1a3202, 0, word), 0x1a3202);
            assert_eq!(interrupt_return_flags(2, 0x20000, word), 2);
            assert_eq!(interrupt_return_flags(2, 0x4000, word), 0x4002);
        }
    }

    #[test]
    fn interrupt_return_word_retains_high_flags_without_loading_frame_rf() {
        assert_eq!(interrupt_return_flags(0x240002, u32::MAX, true), 0x244dd7);
        assert_eq!(interrupt_return_flags(0x240002, 0x10000, true), 0x240002);
        assert_eq!(interrupt_return_flags(2, 0x10000, false), 0x10002);
        assert_eq!(interrupt_return_flags(0x10002, 0, false), 2);
    }
}
