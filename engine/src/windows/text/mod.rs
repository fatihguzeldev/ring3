mod conversion;
pub(crate) mod messages;
pub use conversion::ConversionError32;

mod codepages;
pub use codepages::CodePage32;
use codepages::{CP437, CP1252};
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
