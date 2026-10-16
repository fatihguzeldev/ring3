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
    out.try_reserve(units.len().saturating_mul(3))
        .map_err(|_| NlsError32::Allocation)?;
    let locale: icu_locale::Locale = super::super::locales::locale_identity(locale)?
        .language_tag()
        .parse()
        .map_err(|_| NlsError32::InvalidParameter)?;
    let mapper = icu_casemap::CaseMapper::new();
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut Vec<u16>| {
        if upper {
            out.extend(mapper.uppercase_to_string(run, &locale.id).encode_utf16());
        } else if lower {
            out.extend(mapper.lowercase_to_string(run, &locale.id).encode_utf16());
        } else {
            out.extend(run.encode_utf16());
        }
        run.clear();
    };
    for c in char::decode_utf16(units.iter().copied()) {
        let Ok(mut c) = c else {
            flush(&mut run, &mut out);
            out.push(c.unwrap_err().unpaired_surrogate());
            continue;
        };
        if flags & 0x100000 != 0 && (0x30a1..=0x30f6).contains(&(c as u32)) {
            c = char::from_u32(c as u32 - 0x60).unwrap();
        }
        if flags & 0x200000 != 0 && (0x3041..=0x3096).contains(&(c as u32)) {
            c = char::from_u32(c as u32 + 0x60).unwrap();
        }
        run.push(c);
    }
    flush(&mut run, &mut out);
    if flags & 0xc00000 != 0 {
        super::width::map(&out, flags & 0x800000 != 0)
    } else {
        Ok(out)
    }
}

pub fn map_string(units: &[u16], flags: u32) -> Result<Vec<u16>, NlsError32> {
    map_string_for_locale(units, flags, 0x409)
}
