use crate::windows::nls::NlsError32;
pub fn map_string(units: &[u16], flags: u32) -> Result<Vec<u16>, NlsError32> {
    let upper = flags & 0x200 != 0;
    let lower = flags & 0x100 != 0;
    if upper && lower
        || flags & !0x08f0_0300 != 0
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
        let mut mapped = String::new();
        if upper {
            mapped.extend(c.to_uppercase());
        } else if lower {
            mapped.extend(c.to_lowercase());
        } else {
            mapped.push(c);
        }
        out.extend(mapped.encode_utf16());
    }
    if flags & 0xc00000 != 0 {
        super::width::map(&out, flags & 0x800000 != 0)
    } else {
        Ok(out)
    }
}
