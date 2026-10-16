use super::super::{ConversionError32, TextError32};
use encoding_rs::{EncoderResult, Encoding};

fn encoding(id: u16) -> Option<&'static Encoding> {
    Some(match id {
        874 => encoding_rs::WINDOWS_874,
        932 => encoding_rs::SHIFT_JIS,
        936 => encoding_rs::GBK,
        949 => encoding_rs::EUC_KR,
        950 => encoding_rs::BIG5,
        1250 => encoding_rs::WINDOWS_1250,
        1251 => encoding_rs::WINDOWS_1251,
        1253 => encoding_rs::WINDOWS_1253,
        1254 => encoding_rs::WINDOWS_1254,
        1255 => encoding_rs::WINDOWS_1255,
        1256 => encoding_rs::WINDOWS_1256,
        1257 => encoding_rs::WINDOWS_1257,
        1258 => encoding_rs::WINDOWS_1258,
        10000 => encoding_rs::MACINTOSH,
        10007 => encoding_rs::X_MAC_CYRILLIC,
        20866 => encoding_rs::KOI8_R,
        21866 => encoding_rs::KOI8_U,
        28592 => encoding_rs::ISO_8859_2,
        28593 => encoding_rs::ISO_8859_3,
        28594 => encoding_rs::ISO_8859_4,
        28595 => encoding_rs::ISO_8859_5,
        28596 => encoding_rs::ISO_8859_6,
        28597 => encoding_rs::ISO_8859_7,
        28598 => encoding_rs::ISO_8859_8,
        28603 => encoding_rs::ISO_8859_13,
        28605 => encoding_rs::ISO_8859_15,
        38598 => encoding_rs::ISO_8859_8_I,
        51932 => encoding_rs::EUC_JP,
        54936 => encoding_rs::GB18030,
        _ => return None,
    })
}
pub(super) fn supported(id: u32) -> bool {
    let Ok(id) = u16::try_from(id) else {
        return false;
    };
    matches!(id, 20127 | 28591 | 28599)
        || encoding(id).is_some()
        || oem_cp::code_table::DECODING_TABLE_CP_MAP.contains_key(&id)
}
pub(super) fn decode_byte(id: u16, byte: u8) -> Option<u16> {
    if matches!(id, 932 | 936 | 949 | 950 | 51932 | 54936) {
        return if byte < 0x80 {
            Some(u16::from(byte))
        } else {
            None
        };
    }
    if matches!(id, 28591 | 28599) || id == 20127 && byte < 0x80 {
        return Some(if id == 28599 {
            iso8859_9(byte)
        } else {
            u16::from(byte)
        });
    }
    if let Some(table) = oem_cp::code_table::DECODING_TABLE_CP_MAP.get(&id) {
        return table.decode_char_checked(byte).map(|value| value as u16);
    }
    let decoded = decode(id, &[byte], true).ok()?;
    if decoded.len() == 1 {
        Some(decoded[0])
    } else {
        None
    }
}
pub(super) fn decode(id: u16, bytes: &[u8], strict: bool) -> Result<Vec<u16>, TextError32> {
    if matches!(id, 20127 | 28591 | 28599) {
        return bytes
            .iter()
            .map(|&byte| {
                if id == 20127 && byte >= 0x80 {
                    if strict { Err(TextError32) } else { Ok(0xfffd) }
                } else {
                    Ok(if id == 28599 {
                        iso8859_9(byte)
                    } else {
                        u16::from(byte)
                    })
                }
            })
            .collect();
    }
    if let Some(table) = oem_cp::code_table::DECODING_TABLE_CP_MAP.get(&id) {
        return bytes
            .iter()
            .map(|&byte| {
                table
                    .decode_char_checked(byte)
                    .map(|value| value as u16)
                    .or_else(|| (!strict).then_some(0xfffd))
                    .ok_or(TextError32)
            })
            .collect();
    }
    let mut decoder = encoding(id)
        .ok_or(TextError32)?
        .new_decoder_without_bom_handling();
    let capacity = decoder
        .max_utf16_buffer_length(bytes.len())
        .ok_or(TextError32)?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(capacity)
        .map_err(|_| TextError32)?;
    output.resize(capacity, 0);
    let (result, read, written, errors) = decoder.decode_to_utf16(bytes, &mut output, true);
    if !matches!(result, encoding_rs::CoderResult::InputEmpty)
        || read != bytes.len()
        || strict && errors
    {
        return Err(TextError32);
    }
    output.truncate(written);
    Ok(output)
}
pub(super) fn encode(
    id: u16,
    units: &[u16],
    strict: bool,
    default: &[u8],
) -> Result<(Vec<u8>, bool), TextError32> {
    let mut output = Vec::new();
    output
        .try_reserve_exact(units.len().checked_mul(4).ok_or(TextError32)?)
        .map_err(|_| TextError32)?;
    let mut substituted = false;
    if matches!(id, 20127 | 28591 | 28599)
        || oem_cp::code_table::ENCODING_TABLE_CP_MAP.contains_key(&id)
    {
        for value in char::decode_utf16(units.iter().copied()) {
            let character = value.map_err(|_| TextError32).or_else(|error| {
                if strict {
                    Err(error)
                } else {
                    Ok(char::REPLACEMENT_CHARACTER)
                }
            })?;
            let byte = if id == 20127 {
                character.is_ascii().then_some(character as u8)
            } else if id == 28599 {
                (0..=255u8).find(|&byte| u32::from(iso8859_9(byte)) == character as u32)
            } else if id == 28591 {
                u8::try_from(character as u32).ok()
            } else {
                oem_cp::encode_char_checked(
                    character,
                    oem_cp::code_table::ENCODING_TABLE_CP_MAP.get(&id).unwrap(),
                )
            };
            if let Some(byte) = byte {
                output.push(byte);
            } else {
                if strict {
                    return Err(TextError32);
                }
                substituted = true;
                output.extend_from_slice(default);
            }
        }
        return Ok((output, substituted));
    }
    let mut encoder = encoding(id).ok_or(TextError32)?.new_encoder();
    let mut remaining = units;
    loop {
        let mut buffer = [0; 1024];
        let (result, read, written) =
            encoder.encode_from_utf16_without_replacement(remaining, &mut buffer, true);
        output.extend_from_slice(&buffer[..written]);
        remaining = &remaining[read..];
        match result {
            EncoderResult::InputEmpty => break,
            EncoderResult::OutputFull => {}
            EncoderResult::Unmappable(_) => {
                if strict {
                    return Err(TextError32);
                }
                substituted = true;
                output.extend_from_slice(default);
            }
        }
    }
    Ok((output, substituted))
}
pub(in crate::windows::text) fn encode_windows(
    id: u16,
    units: &[u16],
    flags: u32,
    default: &[u8],
) -> Result<(Vec<u8>, bool), ConversionError32> {
    if id == 54936 {
        if flags & !0x80 != 0 {
            return Err(ConversionError32::InvalidFlags);
        }
        return encode(id, units, flags & 0x80 != 0, default)
            .map_err(|_| ConversionError32::NoTranslation);
    }
    if flags & !0x670 != 0
        || flags & 0x30 == 0x30
        || flags & 0x50 == 0x50
        || flags & 0x60 == 0x60
        || flags & 0x70 != 0 && flags & 0x200 == 0
    {
        return Err(ConversionError32::InvalidFlags);
    }
    use unicode_normalization::UnicodeNormalization;
    let text = String::from_utf16_lossy(units);
    let text = if flags & 0x200 != 0 {
        text.nfc().collect::<String>()
    } else {
        text
    };
    let units: Vec<_> = text.encode_utf16().collect();
    if flags & 0x50 == 0 {
        let (bytes, substituted) =
            encode(id, &units, false, default).map_err(|_| ConversionError32::NoTranslation)?;
        if !substituted
            && (flags & 0x400 == 0
                || decode(id, &bytes, true).is_ok_and(|decoded| decoded == units))
        {
            return Ok((bytes, false));
        }
    }
    let mut output = Vec::new();
    output
        .try_reserve(
            units
                .len()
                .checked_mul(4)
                .ok_or(ConversionError32::Allocation)?,
        )
        .map_err(|_| ConversionError32::Allocation)?;
    let mut substituted = false;
    for character in text.chars() {
        if flags & 0x200 != 0 && unicode_normalization::char::is_combining_mark(character) {
            if flags & 0x10 != 0 {
                continue;
            }
            if flags & 0x40 != 0 {
                output.extend_from_slice(default);
                substituted = true;
                continue;
            }
        }
        let mut scalar = [0; 2];
        let source = character.encode_utf16(&mut scalar);
        if let Ok((bytes, _)) = encode(id, source, true, default)
            && (flags & 0x400 == 0
                || decode(id, &bytes, true).is_ok_and(|decoded| decoded == source))
        {
            output.extend_from_slice(&bytes);
            continue;
        }
        if flags & 0x400 == 0
            && let Some(bytes) = super::bestfit::encode(id, character)
        {
            output.extend_from_slice(if bytes[0] == 0 { &bytes[1..] } else { &bytes });
            continue;
        }
        output.extend_from_slice(default);
        substituted = true;
    }
    Ok((output, substituted))
}

fn iso8859_9(byte: u8) -> u16 {
    match byte {
        0xd0 => 0x11e,
        0xdd => 0x130,
        0xde => 0x15e,
        0xf0 => 0x11f,
        0xfd => 0x131,
        0xfe => 0x15f,
        _ => u16::from(byte),
    }
}
