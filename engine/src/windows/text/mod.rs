#![forbid(unsafe_code)]

const CP1252: [u16; 32] = [
    0x20ac, 0x81, 0x201a, 0x192, 0x201e, 0x2026, 0x2020, 0x2021, 0x2c6, 0x2030, 0x160, 0x2039,
    0x152, 0x8d, 0x17d, 0x8f, 0x90, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014, 0x2dc,
    0x2122, 0x161, 0x203a, 0x153, 0x9d, 0x17e, 0x178,
];
const CP437: [u16; 128] = [
    0xc7, 0xfc, 0xe9, 0xe2, 0xe4, 0xe0, 0xe5, 0xe7, 0xea, 0xeb, 0xe8, 0xef, 0xee, 0xec, 0xc4, 0xc5,
    0xc9, 0xe6, 0xc6, 0xf4, 0xf6, 0xf2, 0xfb, 0xf9, 0xff, 0xd6, 0xdc, 0xa2, 0xa3, 0xa5, 0x20a7,
    0x192, 0xe1, 0xed, 0xf3, 0xfa, 0xf1, 0xd1, 0xaa, 0xba, 0xbf, 0x2310, 0xac, 0xbd, 0xbc, 0xa1,
    0xab, 0xbb, 0x2591, 0x2592, 0x2593, 0x2502, 0x2524, 0x2561, 0x2562, 0x2556, 0x2555, 0x2563,
    0x2551, 0x2557, 0x255d, 0x255c, 0x255b, 0x2510, 0x2514, 0x2534, 0x252c, 0x251c, 0x2500, 0x253c,
    0x255e, 0x255f, 0x255a, 0x2554, 0x2569, 0x2566, 0x2560, 0x2550, 0x256c, 0x2567, 0x2568, 0x2564,
    0x2565, 0x2559, 0x2558, 0x2552, 0x2553, 0x256b, 0x256a, 0x2518, 0x250c, 0x2588, 0x2584, 0x258c,
    0x2590, 0x2580, 0x3b1, 0xdf, 0x393, 0x3c0, 0x3a3, 0x3c3, 0xb5, 0x3c4, 0x3a6, 0x398, 0x3a9,
    0x3b4, 0x221e, 0x3c6, 0x3b5, 0x2229, 0x2261, 0xb1, 0x2265, 0x2264, 0x2320, 0x2321, 0xf7,
    0x2248, 0xb0, 0x2219, 0xb7, 0x221a, 0x207f, 0xb2, 0x25a0, 0xa0,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodePage32 {
    Windows1252,
    Oem437,
    Utf8,
}
impl CodePage32 {
    pub fn from_id(id: u32) -> Option<Self> {
        match id {
            0 | 1252 => Some(Self::Windows1252),
            1 | 437 => Some(Self::Oem437),
            65001 => Some(Self::Utf8),
            _ => None,
        }
    }
    pub fn decode(self, bytes: &[u8], strict: bool) -> Result<Vec<u16>, TextError32> {
        if self == Self::Utf8 {
            return match std::str::from_utf8(bytes) {
                Ok(value) => Ok(value.encode_utf16().collect()),
                Err(_) if strict => Err(TextError32),
                Err(_) => Ok(String::from_utf8_lossy(bytes).encode_utf16().collect()),
            };
        }
        Ok(bytes
            .iter()
            .map(|&byte| match self {
                Self::Windows1252 if (0x80..0xa0).contains(&byte) => {
                    CP1252[usize::from(byte - 0x80)]
                }
                Self::Oem437 if byte >= 0x80 => CP437[usize::from(byte - 0x80)],
                _ => u16::from(byte),
            })
            .collect())
    }
    pub fn encode(self, units: &[u16], strict: bool) -> Result<(Vec<u8>, bool), TextError32> {
        let mut output = Vec::new();
        let mut substituted = false;
        for value in char::decode_utf16(units.iter().copied()) {
            let value = match value {
                Ok(value) => value,
                Err(_) if strict => return Err(TextError32),
                Err(_) => {
                    substituted = true;
                    '�'
                }
            };
            if self == Self::Utf8 {
                let mut bytes = [0; 4];
                output.extend_from_slice(value.encode_utf8(&mut bytes).as_bytes());
                continue;
            }
            let code = value as u32;
            let encoded = match self {
                Self::Windows1252 if code < 0x80 || (0xa0..=0xff).contains(&code) => {
                    Some(code as u8)
                }
                Self::Windows1252 => CP1252
                    .iter()
                    .position(|&unit| u32::from(unit) == code)
                    .map(|index| 0x80 + index as u8),
                Self::Oem437 if code < 0x80 => Some(code as u8),
                Self::Oem437 => CP437
                    .iter()
                    .position(|&unit| u32::from(unit) == code)
                    .map(|index| 0x80 + index as u8),
                _ => unreachable!(),
            };
            output.push(encoded.unwrap_or_else(|| {
                substituted = true;
                b'?'
            }));
        }
        Ok((output, substituted))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextError32;

/// Simple case mapping for ordinal Windows names; expansions such as ß -> SS are excluded.
pub fn ordinal_key(units: &[u16]) -> Vec<u16> {
    let mut result = Vec::with_capacity(units.len());
    for unit in char::decode_utf16(units.iter().copied()) {
        match unit {
            Ok(character) => {
                let mut uppercase = character.to_uppercase();
                let first = uppercase.next().unwrap();
                let mapped = if uppercase.next().is_none() {
                    first
                } else {
                    character
                };
                let mut encoded = [0; 2];
                result.extend_from_slice(mapped.encode_utf16(&mut encoded));
            }
            Err(error) => result.push(error.unpaired_surrogate()),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordinal_names_do_not_expand_or_replace_unpaired_units() {
        assert_eq!(
            ordinal_key(&[b'a' as u16, 0xdf, 0xd800]),
            [b'A' as u16, 0xdf, 0xd800]
        );
        assert_ne!(ordinal_key(&[0xdf]), ordinal_key(&[83, 83]));
    }
    #[test]
    fn byte_profiles_round_trip_all_bytes() {
        for cp in [CodePage32::Windows1252, CodePage32::Oem437] {
            let bytes = (0..=255).collect::<Vec<u8>>();
            let units = cp.decode(&bytes, true).unwrap();
            assert_eq!(cp.encode(&units, true).unwrap(), (bytes, false));
        }
    }
    #[test]
    fn utf8_rejects_invalid_strict_and_substitutes_non_strict() {
        assert_eq!(CodePage32::Utf8.decode(&[0xff], true), Err(TextError32));
        assert_eq!(CodePage32::Utf8.decode(&[0xff], false).unwrap(), [0xfffd]);
        assert!(CodePage32::Utf8.encode(&[0xd800], true).is_err());
        assert_eq!(
            CodePage32::Windows1252
                .encode(&[0xd83d, 0xde00], false)
                .unwrap(),
            (vec![b'?'], true)
        );
    }
}
