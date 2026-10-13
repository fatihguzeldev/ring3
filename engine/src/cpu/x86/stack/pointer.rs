#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StackPointerSize {
    default32: bool,
}

impl StackPointerSize {
    pub(crate) const fn new(default32: bool) -> Self {
        Self { default32 }
    }

    pub(crate) const fn offset(self, esp: u32) -> u32 {
        if self.default32 { esp } else { esp & 0xffff }
    }

    pub(crate) const fn assign(self, esp: u32, offset: u32) -> u32 {
        if self.default32 {
            offset
        } else {
            (esp & !0xffff) | (offset & 0xffff)
        }
    }

    pub(crate) const fn advance(self, esp: u32, bytes: i32) -> u32 {
        self.assign(esp, self.offset(esp).wrapping_add_signed(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn implicit_pointer_width_preserves_high_esp_and_wraps_only_active_pointer() {
        let narrow = StackPointerSize::new(false);
        let wide = StackPointerSize::new(true);
        for (original, delta, b16, b32) in [
            (0xabcd_0024, -4, 0xabcd_0020, 0xabcd_0020),
            (0xabcd_0000, -2, 0xabcd_fffe, 0xabcc_fffe),
            (0xabcd_fffe, 2, 0xabcd_0000, 0xabce_0000),
            (0xabcd_fffc, 12, 0xabcd_0008, 0xabce_0008),
        ] {
            assert_eq!(narrow.advance(original, delta), b16);
            assert_eq!(wide.advance(original, delta), b32);
        }
        assert_eq!(narrow.offset(0xabcd_0020), 0x20);
        assert_eq!(wide.offset(0xabcd_0020), 0xabcd_0020);
    }

    #[test]
    fn leave_pointer_assignment_and_pop_sp_order_do_not_share_widths() {
        let narrow = StackPointerSize::new(false);
        let wide = StackPointerSize::new(true);
        assert_eq!(narrow.assign(0xabcd_0020, 0x7654_1200), 0xabcd_1200);
        assert_eq!(wide.assign(0xabcd_0020, 0x7654_1200), 0x7654_1200);
        // pop SP increments the active stack pointer before the low destination write.
        let old = 0xabcd_fffe;
        let popped_word = 0x1234;
        assert_eq!(
            (narrow.advance(old, 2) & !0xffff) | popped_word,
            0xabcd_1234
        );
        assert_eq!((wide.advance(old, 2) & !0xffff) | popped_word, 0xabce_1234);
    }
}
