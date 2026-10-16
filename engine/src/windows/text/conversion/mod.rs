mod bestfit;
use super::{CP437, CP1252, CodePage32};
use unicode_normalization::UnicodeNormalization;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConversionError32 {
    InvalidFlags,
    NoTranslation,
    Allocation,
}
impl CodePage32 {
    pub const fn id(self) -> u32 {
        match self {
            Self::Windows1252 => 1252,
            Self::Oem437 => 437,
            Self::Utf8 => 65001,
            Self::Legacy(id) => id as u32,
        }
    }
    pub fn decode_windows(self, bytes: &[u8], flags: u32) -> Result<Vec<u16>, ConversionError32> {
        if matches!(self, Self::Utf8 | Self::Legacy(54936)) {
            if flags & !8 != 0 {
                return Err(ConversionError32::InvalidFlags);
            }
            return self
                .decode(bytes, flags & 8 != 0)
                .map_err(|_| ConversionError32::NoTranslation);
        }
        if flags & !0xf != 0 || flags & 3 == 3 {
            return Err(ConversionError32::InvalidFlags);
        }
        let source = self
            .decode(bytes, flags & 8 != 0)
            .map_err(|_| ConversionError32::NoTranslation)?;
        if flags & 2 == 0 {
            return Ok(source);
        }
        let text = String::from_utf16(&source).map_err(|_| ConversionError32::NoTranslation)?;
        let mut output = Vec::new();
        output
            .try_reserve_exact(source.len().saturating_mul(3))
            .map_err(|_| ConversionError32::Allocation)?;
        for c in text.nfd() {
            let mut units = [0; 2];
            output.extend_from_slice(c.encode_utf16(&mut units));
        }
        Ok(output)
    }
    pub fn encode_windows(
        self,
        units: &[u16],
        flags: u32,
        default: u8,
    ) -> Result<(Vec<u8>, bool), ConversionError32> {
        if self == Self::Utf8 {
            if flags & !0x80 != 0 {
                return Err(ConversionError32::InvalidFlags);
            }
            return self
                .encode(units, flags & 0x80 != 0)
                .map_err(|_| ConversionError32::NoTranslation);
        }
        if let Self::Legacy(id) = self {
            return super::codepages::legacy::encode_windows(id, units, flags, &[default]);
        }
        if flags & !0x670 != 0
            || flags & 0x30 == 0x30
            || flags & 0x50 == 0x50
            || flags & 0x60 == 0x60
            || flags & 0x70 != 0 && flags & 0x200 == 0
        {
            return Err(ConversionError32::InvalidFlags);
        }
        let mut output = Vec::new();
        output
            .try_reserve_exact(units.len())
            .map_err(|_| ConversionError32::Allocation)?;
        let mut substituted = false;
        let text = String::from_utf16_lossy(units);
        let text = if flags & 0x200 != 0 {
            text.nfc().collect::<String>()
        } else {
            text
        };
        for value in text.chars() {
            if flags & 0x200 != 0 && unicode_normalization::char::is_combining_mark(value) {
                if flags & 0x10 != 0 {
                    continue;
                }
                if flags & 0x40 != 0 {
                    output.push(default);
                    substituted = true;
                    continue;
                }
            }
            let code = value as u32;
            let exact = match self {
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
                Self::Utf8 | Self::Legacy(_) => unreachable!(),
            };
            let best = if flags & 0x400 == 0 {
                if self == Self::Windows1252 {
                    u16::try_from(code).ok().and_then(|code| {
                        bestfit::WINDOWS_1252
                            .binary_search_by_key(&code, |pair| pair.0)
                            .ok()
                            .map(|at| bestfit::WINDOWS_1252[at].1)
                    })
                } else {
                    let mut bytes = [0; 4];
                    let mut decomposed = value.encode_utf8(&mut bytes).nfkd();
                    decomposed.next().filter(|c| c.is_ascii()).map(|c| c as u8)
                }
            } else {
                None
            };
            if let Some(byte) = exact.or(best) {
                output.push(byte);
            } else {
                output.push(default);
                substituted = true;
            }
        }
        Ok((output, substituted))
    }
}
