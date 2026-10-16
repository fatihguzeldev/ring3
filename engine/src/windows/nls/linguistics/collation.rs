use crate::windows::nls::NlsError32;
use icu_collator::{
    Collator, CollatorBorrowed, CollatorPreferences,
    options::{AlternateHandling, CaseLevel, CollatorOptions, Strength},
    preferences::CollationNumericOrdering,
};
use std::cmp::Ordering;
use unicode_general_category::{GeneralCategory as G, get_general_category};
const FLAGS: u32 = 0x0803_103f;
fn collator(flags: u32, locale: u32) -> Result<CollatorBorrowed<'static>, NlsError32> {
    if flags & !FLAGS != 0 {
        return Err(NlsError32::InvalidFlags);
    }
    let ignore_case = flags & (1 | 0x10) != 0;
    let ignore_marks = flags & (2 | 0x20) != 0;
    let mut options = CollatorOptions::default();
    options.strength = Some(if ignore_marks {
        Strength::Primary
    } else if ignore_case {
        Strength::Secondary
    } else {
        if flags & 0x1000 == 0 {
            Strength::Quaternary
        } else {
            Strength::Tertiary
        }
    });
    options.case_level = Some(if ignore_marks && !ignore_case {
        CaseLevel::On
    } else {
        CaseLevel::Off
    });
    options.alternate_handling = Some(if flags & 0x1000 == 0 {
        AlternateHandling::Shifted
    } else {
        AlternateHandling::NonIgnorable
    });
    let locale: icu_locale::Locale = super::super::locales::locale_identity(locale)?
        .language_tag()
        .parse()
        .map_err(|_| NlsError32::InvalidParameter)?;
    let mut preferences: CollatorPreferences = locale.into();
    preferences.numeric_ordering = Some(if flags & 8 != 0 {
        CollationNumericOrdering::True
    } else {
        CollationNumericOrdering::False
    });
    Collator::try_new(preferences, options).map_err(|_| NlsError32::Unsupported)
}
fn prepared(units: &[u16], flags: u32) -> Result<Vec<u16>, NlsError32> {
    let mut output = Vec::new();
    output
        .try_reserve(units.len().saturating_mul(3))
        .map_err(|_| NlsError32::Allocation)?;
    let normalized = if flags & 0x20000 != 0 {
        super::width::normalized(units)?
    } else {
        units.to_vec()
    };
    for value in char::decode_utf16(normalized.iter().copied()) {
        let mut value = value.unwrap_or(char::REPLACEMENT_CHARACTER);
        if flags & 4 != 0
            && matches!(
                get_general_category(value),
                G::ClosePunctuation
                    | G::ConnectorPunctuation
                    | G::DashPunctuation
                    | G::FinalPunctuation
                    | G::InitialPunctuation
                    | G::OpenPunctuation
                    | G::OtherPunctuation
                    | G::MathSymbol
                    | G::CurrencySymbol
                    | G::ModifierSymbol
                    | G::OtherSymbol
            )
        {
            continue;
        }
        if flags & 0x10000 != 0 && (0x30a1..=0x30f6).contains(&(value as u32)) {
            value = char::from_u32(value as u32 - 0x60).unwrap();
        }
        let mut units = [0; 2];
        output.extend_from_slice(value.encode_utf16(&mut units));
    }
    Ok(output)
}
pub fn compare_for_locale(
    left: &[u16],
    right: &[u16],
    flags: u32,
    locale: u32,
) -> Result<Ordering, NlsError32> {
    let collator = collator(flags, locale)?;
    let left = prepared(left, flags)?;
    let right = prepared(right, flags)?;
    Ok(collator.compare_utf16(&left, &right))
}
pub fn sort_key_for_locale(units: &[u16], flags: u32, locale: u32) -> Result<Vec<u8>, NlsError32> {
    let collator = collator(flags, locale)?;
    let units = prepared(units, flags)?;
    let capacity = units
        .len()
        .checked_mul(32)
        .and_then(|value| value.checked_add(64))
        .ok_or(NlsError32::Capacity)?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(capacity)
        .map_err(|_| NlsError32::Allocation)?;
    output.resize(capacity, 0);
    let written = collator
        .write_sort_key_utf16_to(&units, &mut output[..])
        .map_err(|_| NlsError32::Capacity)?;
    output.truncate(written);
    output.push(0);
    Ok(output)
}

pub fn compare(left: &[u16], right: &[u16], flags: u32) -> Result<Ordering, NlsError32> {
    compare_for_locale(left, right, flags, 0x409)
}
pub fn sort_key(units: &[u16], flags: u32) -> Result<Vec<u8>, NlsError32> {
    sort_key_for_locale(units, flags, 0x409)
}
