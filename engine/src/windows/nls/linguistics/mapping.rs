use crate::windows::nls::NlsError32;
pub fn map_string_for_locale(
    units: &[u16],
    flags: u32,
    locale: u32,
) -> Result<Vec<u16>, NlsError32> {
    let upper = flags & 0x200 != 0;
    let lower = flags & 0x100 != 0;
    if upper && lower
        || flags & !0x09f0_0300 != 0
        || flags & 0x100000 != 0 && flags & 0x200000 != 0
        || flags & 0x400000 != 0 && flags & 0x800000 != 0
    {
        return Err(NlsError32::InvalidFlags);
    }
    if !upper && !lower && flags & 0xf00000 == 0 {
        return Err(NlsError32::InvalidFlags);
    }
    let mut out = Vec::new();
    out.try_reserve(units.len().saturating_mul(2))
        .map_err(|_| NlsError32::Allocation)?;
    let identity = super::super::locales::locale_identity(locale)?;
    let linguistic = flags & 0x0100_0000 != 0;
    let turkic = linguistic && matches!(identity.language, "tr" | "az");
    let mapper = icu_casemap::CaseMapper::new();
    for c in char::decode_utf16(units.iter().copied()) {
        let Ok(mut c) = c else {
            out.push(c.unwrap_err().unpaired_surrogate());
            continue;
        };
        if flags & 0x100000 != 0 && (0x30a1..=0x30f6).contains(&(c as u32)) {
            c = char::from_u32(c as u32 - 0x60).unwrap();
        }
        if flags & 0x200000 != 0 && (0x3041..=0x3096).contains(&(c as u32)) {
            c = char::from_u32(c as u32 + 0x60).unwrap();
        }
        // windows maps each scalar without expanding or contextual casing.
        // its default filesystem mode preserves the dotted/dotless I pair.
        if (upper || lower) && (linguistic || !matches!(c, 'İ' | 'ı')) {
            c = if upper {
                if turkic && c == 'i' {
                    'İ'
                } else {
                    mapper.simple_uppercase(c)
                }
            } else if turkic && c == 'I' {
                'ı'
            } else {
                mapper.simple_lowercase(c)
            };
        }
        out.extend_from_slice(c.encode_utf16(&mut [0; 2]));
    }
    if flags & 0xc00000 != 0 {
        super::width::map(&out, flags & 0x800000 != 0)
    } else {
        Ok(out)
    }
}

pub fn map_string(units: &[u16], flags: u32) -> Result<Vec<u16>, NlsError32> {
    map_string_for_locale(units, flags, 0x409)
}
