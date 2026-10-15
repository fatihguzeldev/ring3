pub fn virtual_key(scan: u16) -> u8 {
    match scan {
        1 => 0x1b,
        2..=10 => b'1' + scan as u8 - 2,
        11 => b'0',
        12 => 0xbd,
        13 => 0xbb,
        14 => 8,
        15 => 9,
        16 => b'Q',
        17 => b'W',
        18 => b'E',
        19 => b'R',
        20 => b'T',
        21 => b'Y',
        22 => b'U',
        23 => b'I',
        24 => b'O',
        25 => b'P',
        26 => 0xdb,
        27 => 0xdd,
        28 => 13,
        29 => 0xa2,
        30 => b'A',
        31 => b'S',
        32 => b'D',
        33 => b'F',
        34 => b'G',
        35 => b'H',
        36 => b'J',
        37 => b'K',
        38 => b'L',
        39 => 0xba,
        40 => 0xde,
        41 => 0xc0,
        42 => 0xa0,
        43 => 0xdc,
        44 => b'Z',
        45 => b'X',
        46 => b'C',
        47 => b'V',
        48 => b'B',
        49 => b'N',
        50 => b'M',
        51 => 0xbc,
        52 => 0xbe,
        53 => 0xbf,
        54 => 0xa1,
        55 => 0x6a,
        56 => 0xa4,
        57 => 32,
        58 => 0x14,
        59..=68 => 0x70 + scan as u8 - 59,
        69 => 0x90,
        70 => 0x91,
        71 => 0x67,
        72 => 0x68,
        73 => 0x69,
        74 => 0x6d,
        75 => 0x64,
        76 => 0x65,
        77 => 0x66,
        78 => 0x6b,
        79 => 0x61,
        80 => 0x62,
        81 => 0x63,
        82 => 0x60,
        83 => 0x6e,
        86 => 0xe2,
        87 => 0x7a,
        88 => 0x7b,
        0x64..=0x6f => 0x7c + scan as u8 - 0x64,
        0x90 => 0xb1,
        0x99 => 0xb0,
        0x9c => 13,
        0x9d => 0xa3,
        0xa0 => 0xad,
        0xa1 => 0xb7,
        0xa2 => 0xb3,
        0xa4 => 0xb2,
        0xae => 0xae,
        0xb0 => 0xaf,
        0xb2 => 0xac,
        0xb5 => 0x6f,
        0xb7 => 0x2c,
        0xb8 => 0xa5,
        0xc5 => 0x13,
        0xc7 => 0x24,
        0xc8 => 0x26,
        0xc9 => 0x21,
        0xcb => 0x25,
        0xcd => 0x27,
        0xcf => 0x23,
        0xd0 => 0x28,
        0xd1 => 0x22,
        0xd2 => 0x2d,
        0xd3 => 0x2e,
        0xdb => 0x5b,
        0xdc => 0x5c,
        0xdd => 0x5d,
        0xde => 0x5e,
        0xdf => 0x5f,
        0xe5 => 0xaa,
        0xe6 => 0xa9,
        0xe7 => 0xa8,
        0xe8 => 0xa7,
        0xe9 => 0xa6,
        0xeb => 0xb6,
        0xec => 0xb4,
        0xed => 0xb5,
        _ => 0,
    }
}

pub fn scan_code(key: u32) -> Option<u16> {
    let key = match key {
        0x10 => 0xa0,
        0x11 => 0xa2,
        0x12 => 0xa4,
        _ => u8::try_from(key).ok()?,
    };
    (1..=255).find(|scan| virtual_key(*scan) == key)
}

pub fn map_virtual_key(code: u32, kind: u32) -> u32 {
    map_virtual_key_for_layout(super::KeyboardLayout32::Us, code, kind)
}

pub fn key_name(scan: u16) -> String {
    match scan {
        0x1 => "Escape",
        0xe => "Backspace",
        0xf => "Tab",
        0x1c => "Enter",
        0x9c => "Num Enter",
        0x1d => "Left Ctrl",
        0x9d => "Right Ctrl",
        0x2a => "Left Shift",
        0x36 => "Right Shift",
        0x38 => "Left Alt",
        0xb8 => "Right Alt",
        0x39 => "Space",
        0x3a => "Caps Lock",
        0x45 => "Num Lock",
        0x46 => "Scroll Lock",
        0xb7 => "Print Screen",
        0xc5 => "Pause",
        0xc7 => "Home",
        0xc8 => "Up",
        0xc9 => "Page Up",
        0xcb => "Left",
        0xcd => "Right",
        0xcf => "End",
        0xd0 => "Down",
        0xd1 => "Page Down",
        0xd2 => "Insert",
        0xd3 => "Delete",
        0xdb => "Left Windows",
        0xdc => "Right Windows",
        0xdd => "Application",
        _ => {
            return match virtual_key(scan) {
                b'A'..=b'Z' | b'0'..=b'9' => (virtual_key(scan) as char).to_string(),
                0x70..=0x87 => format!("F{}", virtual_key(scan) - 0x6f),
                0x60..=0x69 => format!("Num {}", virtual_key(scan) - 0x60),
                _ => format!("Key {scan:02X}"),
            };
        }
    }
    .to_owned()
}

pub fn virtual_key_for_layout(layout: super::KeyboardLayout32, scan: u16) -> u8 {
    if layout == super::KeyboardLayout32::TurkishQ {
        match scan {
            0x0c => return 0xdf,
            0x0d => return 0xbd,
            0x2b => return 0xbc,
            0x33 => return 0xbf,
            0x34 => return 0xdc,
            0x35 => return 0xbe,
            _ => {}
        }
    }
    virtual_key(scan)
}

pub fn map_virtual_key_for_layout(layout: super::KeyboardLayout32, code: u32, kind: u32) -> u32 {
    if matches!(kind, 0 | 4) {
        match code {
            0x13 => return if kind == 4 { 0xe11d } else { 0x45 },
            3 => return if kind == 4 { 0xe046 } else { 0x46 },
            0x2c => return 0x54,
            _ => {}
        }
        let key = match code {
            0x10 => 0xa0,
            0x11 => 0xa2,
            0x12 => 0xa4,
            _ => match u8::try_from(code) {
                Ok(key) => key,
                Err(_) => return 0,
            },
        };
        let Some(scan) = (1..=255).find(|&scan| virtual_key_for_layout(layout, scan) == key) else {
            return 0;
        };
        let extended = matches!(key, 0xa3 | 0xa5 | 0x5b | 0x5c | 0x5d | 0x6f);
        return if kind == 4 && extended {
            0xe000 | u32::from(scan & 0x7f)
        } else {
            u32::from(scan & 0x7f)
        };
    }
    if matches!(kind, 1 | 3) {
        if code > 0xffff {
            return 0;
        }
        let key = if code & 0xff00 == 0xe100 {
            if code & 0xff == 0x1d { 0x13 } else { 0 }
        } else if code & 0xff00 == 0xe000 {
            if code & 0xff == 0x46 {
                3
            } else if matches!(code&0xff,0x1c|0x1d|0x35|0x37|0x38|0x47..=0x49|0x4b|0x4d|0x4f..=0x53|0x5b..=0x5d)
            {
                virtual_key_for_layout(layout, (code as u16 & 0x7f) | 0x80)
            } else {
                0
            }
        } else if code <= 0x7f {
            if code == 0x54 {
                0x2c
            } else {
                match virtual_key_for_layout(layout, code as u16) {
                    0x60 => 0x2d,
                    0x61 => 0x23,
                    0x62 => 0x28,
                    0x63 => 0x22,
                    0x64 => 0x25,
                    0x65 => 0x0c,
                    0x66 => 0x27,
                    0x67 => 0x24,
                    0x68 => 0x26,
                    0x69 => 0x21,
                    0x6e => 0x2e,
                    key => key,
                }
            }
        } else {
            0
        };
        return u32::from(if kind == 1 {
            match key {
                0xa0 | 0xa1 => 0x10,
                0xa2 | 0xa3 => 0x11,
                0xa4 | 0xa5 => 0x12,
                _ => key,
            }
        } else {
            key
        });
    }
    0
}
