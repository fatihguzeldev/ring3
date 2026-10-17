#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Hotkey32 {
    pub key: u16,
    pub font: u32,
    pub invalid: u8,
    pub defaults: u8,
    pub modifiers: u8,
}

impl Hotkey32 {
    pub fn key_down(&mut self, key: u8, extended: bool) {
        self.key = 0;
        match key {
            8 | 9 | 13 | 27 | 32 | 46 => {}
            0x10 | 0xa0 | 0xa1 => self.modifiers |= 1,
            0x11 | 0xa2 | 0xa3 => self.modifiers |= 2,
            0x12 | 0xa4 | 0xa5 => self.modifiers |= 4,
            _ => {
                let mut modifiers = self.modifiers;
                let invalid = [1, 2, 4, 16, 8, 32, 64, 128][usize::from(modifiers & 7)];
                if self.invalid & invalid != 0 {
                    modifiers |= self.defaults;
                }
                self.key =
                    u16::from(key) | u16::from(modifiers | if extended { 8 } else { 0 }) << 8;
            }
        }
    }
    pub fn key_up(&mut self, key: u8) {
        self.modifiers &= !match key {
            0x10 | 0xa0 | 0xa1 => 1,
            0x11 | 0xa2 | 0xa3 => 2,
            0x12 | 0xa4 | 0xa5 => 4,
            _ => 0,
        };
    }
    pub fn label(&self, layout: crate::input::keyboard::KeyboardLayout32) -> Vec<u16> {
        if self.key == 0 && self.modifiers == 0 {
            return "None".encode_utf16().collect();
        }
        let modifiers = if self.key == 0 {
            self.modifiers
        } else {
            (self.key >> 8) as u8
        };
        let mut text = String::new();
        for (mask, name) in [(2, "Ctrl + "), (1, "Shift + "), (4, "Alt + ")] {
            if modifiers & mask != 0 {
                text.push_str(name);
            }
        }
        let key = self.key as u8;
        if key != 0 {
            let scan =
                crate::input::keyboard::map_virtual_key_for_layout(layout, u32::from(key), 4);
            let mut scan = (scan & 0x7f) as u16
                | if modifiers & 8 != 0 || scan & 0xff00 != 0 {
                    0x80
                } else {
                    0
                };
            if crate::input::keyboard::virtual_key_for_layout(layout, scan) != key
                && crate::input::keyboard::virtual_key_for_layout(layout, scan & 0x7f) == key {
                scan &= 0x7f;
            }
            text.push_str(&crate::input::keyboard::key_name(scan));
        }
        text.encode_utf16().collect()
    }
}

#[cfg(test)]
mod tests;
