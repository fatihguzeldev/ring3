use super::Accelerator32;

impl Accelerator32 {
    pub(crate) fn matches(self, message: u32, key: u32, modifiers: u8) -> bool {
        if key != u32::from(self.key) {
            return false;
        }
        if self.flags & 1 != 0 {
            matches!(message, 0x100 | 0x104) && modifiers & 0x1c == self.flags & 0x1c
        } else {
            match message {
                0x102 => self.flags & 0x10 == 0,
                0x106 => self.flags & 0x10 != 0,
                _ => false,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_modifier_and_character_message_rows_keep_their_distinct_results() {
        let rows = [
            (9, 0x100, 65, 8, true),
            (9, 0x100, 65, 0, false),
            (1, 0x100, 65, 4, false),
            (0, 0x102, 120, 0, true),
            (8, 0x102, 120, 0, true),
            (8, 0x102, 120, 8, true),
            (0, 0x100, 120, 0, false),
            (0, 0x106, 120, 16, false),
            (16, 0x106, 120, 16, true),
        ];
        for (flags, message, key, modifiers, expected) in rows {
            let entry = Accelerator32 {
                flags,
                key,
                command: 9,
            };
            assert_eq!(
                entry.matches(message, u32::from(key), modifiers),
                expected,
                "flags={flags}, message={message}, modifiers={modifiers}"
            );
            assert!(!entry.matches(message, u32::from(key) + 1, modifiers));
        }
    }

    #[test]
    fn matching_ignores_retained_nonsemantic_bits_without_folding_key_or_message_width() {
        let entry = Accelerator32 {
            flags: 0x41,
            key: 65,
            command: 7,
        };
        assert!(entry.matches(0x100, 65, 0));
        assert!(!entry.matches(0x100, 0x10041, 0));
        assert!(!entry.matches(0x101, 65, 0));
        assert!(!entry.matches(0x102, 65, 0));
    }
}
