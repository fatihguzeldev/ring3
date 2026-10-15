#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeyboardLayout32 {
    #[default]
    Us,
    TurkishQ,
}

impl KeyboardLayout32 {
    pub fn handle(self) -> u32 {
        match self {
            Self::Us => 0x0409_0409,
            Self::TurkishQ => 0x041f_041f,
        }
    }
    pub fn from_handle(handle: u32) -> Option<Self> {
        match handle {
            0 | 0x409 | 0x0409_0409 => Some(Self::Us),
            0x41f | 0x041f_041f => Some(Self::TurkishQ),
            _ => None,
        }
    }
}

pub fn character(
    layout: KeyboardLayout32,
    key: u8,
    shift: bool,
    caps: bool,
    control: bool,
    alt: bool,
) -> Option<char> {
    if control && alt {
        if layout != KeyboardLayout32::TurkishQ || shift {
            return None;
        }
        return match key {
            b'1' => Some('>'),
            b'2' => Some('£'),
            b'3' => Some('#'),
            b'4' => Some('$'),
            b'5' => Some('½'),
            b'7' => Some('{'),
            b'8' => Some('['),
            b'9' => Some(']'),
            b'0' => Some('}'),
            0xdf => Some('\\'),
            0xbd => Some('|'),
            b'Q' => Some('@'),
            b'E' => Some('€'),
            b'T' => Some('₺'),
            b'I' => Some('i'),
            b'A' => Some('æ'),
            b'S' => Some('ß'),
            0xdb => Some('¨'),
            0xdd => Some('~'),
            0xba => Some('´'),
            0xbc => Some('`'),
            0xc0 => Some('<'),
            0xe2 => Some('|'),
            _ => None,
        };
    }
    if control {
        return match key {
            b'A'..=b'Z' => char::from_u32(u32::from(key - b'A' + 1)),
            8 => Some('\u{7f}'),
            13 => Some('\n'),
            0xdb => Some('\u{1b}'),
            0xdc => Some('\u{1c}'),
            0xdd => Some('\u{1d}'),
            b'6' if shift => Some('\u{1e}'),
            0xbd if shift => Some('\u{1f}'),
            b'2' if shift => Some('\0'),
            _ => None,
        };
    }
    if layout == KeyboardLayout32::TurkishQ {
        let pair = match key {
            b'I' => Some(('ı', 'I')),
            0xba => Some(('ş', 'Ş')),
            0xde => Some(('i', 'İ')),
            0xdb => Some(('ğ', 'Ğ')),
            0xdd => Some(('ü', 'Ü')),
            0xbf => Some(('ö', 'Ö')),
            0xdc => Some(('ç', 'Ç')),
            _ => None,
        };
        if let Some((plain, upper)) = pair {
            return Some(if shift ^ caps { upper } else { plain });
        }
        let pair = match key {
            0xc0 => Some(('"', 'é')),
            0xdf => Some(('*', '?')),
            0xbd => Some(('-', '_')),
            0xbc => Some((',', ';')),
            0xbe => Some(('.', ':')),
            _ => None,
        };
        if let Some((plain, upper)) = pair {
            return Some(if shift { upper } else { plain });
        }
    }
    if key.is_ascii_uppercase() {
        return Some(if shift ^ caps {
            key as char
        } else {
            (key + 32) as char
        });
    }
    if key.is_ascii_digit() {
        let upper = if layout == KeyboardLayout32::TurkishQ {
            ['=', '!', '\'', '^', '+', '%', '&', '/', '(', ')']
        } else {
            [')', '!', '@', '#', '$', '%', '^', '&', '*', '(']
        };
        return Some(if shift {
            upper[(key - b'0') as usize]
        } else {
            key as char
        });
    }
    let (plain, upper) = match key {
        8 => ('\u{8}', '\u{8}'),
        9 => ('\t', '\t'),
        13 => ('\r', '\r'),
        32 => (' ', ' '),
        0x60..=0x69 => return Some((b'0' + key - 0x60) as char),
        0x6a => ('*', '*'),
        0x6b => ('+', '+'),
        0x6d => ('-', '-'),
        0x6e => ('.', '.'),
        0x6f => ('/', '/'),
        0xba => (';', ':'),
        0xbb => ('=', '+'),
        0xbc => (',', '<'),
        0xbd => ('-', '_'),
        0xbe => ('.', '>'),
        0xbf => ('/', '?'),
        0xc0 => ('`', '~'),
        0xdb => ('[', '{'),
        0xdc => ('\\', '|'),
        0xdd => (']', '}'),
        0xde => ('\'', '"'),
        0xe2 => ('<', '>'),
        _ => return None,
    };
    Some(if shift { upper } else { plain })
}

pub fn dead_key(layout: KeyboardLayout32, key: u8, shift: bool, control: bool, alt: bool) -> bool {
    layout == KeyboardLayout32::TurkishQ
        && ((key == b'3' && shift && !control && !alt)
            || (!shift && control && alt && matches!(key, 0xdb | 0xdd | 0xba | 0xbc)))
}

pub fn compose_dead(accent: char, character: char) -> Option<char> {
    if character == ' ' || character == accent {
        return Some(accent);
    }
    let plain = "aAeEiIıIoOuUnNyY";
    let composed = match accent {
        '^' => "âÂêÊîÎîÎôÔûÛ\0\0\0\0",
        '¨' => "äÄëËïÏïÏöÖüÜ\0\0ÿŸ",
        '´' => "áÁéÉíÍíÍóÓúÚ\0\0ýÝ",
        '`' => "àÀèÈìÌìÌòÒùÙ\0\0\0\0",
        '~' => "ãÃ\0\0\0\0\0\0õÕ\0\0ñÑ\0\0",
        _ => return None,
    };
    plain
        .chars()
        .zip(composed.chars())
        .find(|&(key, value)| key == character && value != '\0')
        .map(|(_, value)| value)
}

pub fn encode_ansi(layout: KeyboardLayout32, character: char) -> Option<u8> {
    if layout == KeyboardLayout32::TurkishQ {
        match character {
            'Ğ' => return Some(0xd0),
            'İ' => return Some(0xdd),
            'Ş' => return Some(0xde),
            'ğ' => return Some(0xf0),
            'ı' => return Some(0xfd),
            'ş' => return Some(0xfe),
            _ => {}
        }
    }
    if character as u32 <= 0xff {
        return Some(character as u8);
    }
    match character {
        '€' => Some(0x80),
        '‘' => Some(0x91),
        '’' => Some(0x92),
        '“' => Some(0x93),
        '”' => Some(0x94),
        '–' => Some(0x96),
        '—' => Some(0x97),
        _ => None,
    }
}

pub fn decode_ansi(layout: KeyboardLayout32, value: u8) -> char {
    if layout == KeyboardLayout32::TurkishQ {
        match value {
            0xd0 => return 'Ğ',
            0xdd => return 'İ',
            0xde => return 'Ş',
            0xf0 => return 'ğ',
            0xfd => return 'ı',
            0xfe => return 'ş',
            _ => {}
        }
    }
    match value {
        0x80 => '€',
        0x91 => '‘',
        0x92 => '’',
        0x93 => '“',
        0x94 => '”',
        0x96 => '–',
        0x97 => '—',
        _ => value as char,
    }
}
