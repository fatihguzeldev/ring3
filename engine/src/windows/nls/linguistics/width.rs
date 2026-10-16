use crate::windows::nls::NlsError32;
use unicode_normalization::UnicodeNormalization;
fn half(character: char) -> Option<char> {
    (0xff61..=0xff9f).filter_map(char::from_u32).find(|&half| {
        let mut encoded = [0; 4];
        let mut normal = half.encode_utf8(&mut encoded).nfkc();
        normal.next() == Some(character) && normal.next().is_none()
    })
}
pub(super) fn map(units: &[u16], full: bool) -> Result<Vec<u16>, NlsError32> {
    let mut output = Vec::new();
    output
        .try_reserve_exact(units.len().saturating_mul(2))
        .map_err(|_| NlsError32::Allocation)?;
    let mut characters = char::decode_utf16(units.iter().copied()).peekable();
    while let Some(value) = characters.next() {
        let Ok(character) = value else {
            output.push(value.unwrap_err().unpaired_surrogate());
            continue;
        };
        let mut mapped = String::new();
        if full {
            match character {
                ' ' => mapped.push('\u{3000}'),
                '!'..='~' => mapped.push(char::from_u32(character as u32 + 0xfee0).unwrap()),
                '\u{ff61}'..='\u{ffdc}' => {
                    mapped.push(character);
                    if matches!(characters.peek(), Some(Ok('\u{ff9e}' | '\u{ff9f}'))) {
                        mapped.push(characters.next().unwrap().unwrap());
                    }
                    mapped = mapped.nfkc().collect();
                }
                _ => mapped.push(character),
            }
        } else {
            match character {
                '\u{3000}' => mapped.push(' '),
                '\u{ff01}'..='\u{ff5e}' => {
                    mapped.push(char::from_u32(character as u32 - 0xfee0).unwrap())
                }
                _ => {
                    if let Some(character) = half(character) {
                        mapped.push(character);
                    } else {
                        let mut encoded = [0; 4];
                        let decomposed = character
                            .encode_utf8(&mut encoded)
                            .nfd()
                            .collect::<Vec<_>>();
                        if decomposed.len() == 2
                            && let (Some(base), Some(mark)) =
                                (half(decomposed[0]), half(decomposed[1]))
                        {
                            mapped.push(base);
                            mapped.push(mark);
                        } else {
                            mapped.push(character);
                        }
                    }
                }
            }
        }
        output.extend(mapped.encode_utf16());
    }
    Ok(output)
}
pub(super) fn normalized(units: &[u16]) -> Result<Vec<u16>, NlsError32> {
    let mut units = map(units, true)?;
    for unit in &mut units {
        match *unit {
            0x3000 => *unit = 0x20,
            0xff01..=0xff5e => *unit -= 0xfee0,
            _ => {}
        }
    }
    Ok(units)
}
