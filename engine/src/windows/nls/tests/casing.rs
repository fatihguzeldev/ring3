use crate::windows::nls::linguistics::{compare_for_locale, sort_key_for_locale};
use std::cmp::Ordering;
#[test]
fn native_windows_default_and_linguistic_casing_keep_distinct_i_family_equivalences() {
    // authored x86 windows oracle e980711b, run 38959995992.
    let points = [0x49u16, 0x69, 0x130, 0x131];
    let default_equal = [0b0011, 0b0011, 0b0100, 0b1000];
    let turkish_linguistic_equal = [0b1001, 0b0110, 0b0110, 0b1001];
    for locale in [0x409, 0x41f, 0x411, 0x41d] {
        for flags in [1, 0x10, 0x08000001, 0x08000010] {
            let expected = if locale == 0x41f && flags & 0x08000000 != 0 {
                turkish_linguistic_equal
            } else {
                default_equal
            };
            for (left_index, left) in points.into_iter().enumerate() {
                for (right_index, right) in points.into_iter().enumerate() {
                    let equal = expected[left_index] & (1 << right_index) != 0;
                    let ordering = compare_for_locale(&[left], &[right], flags, locale).unwrap();
                    assert_eq!(
                        ordering == Ordering::Equal,
                        equal,
                        "locale {locale:x} flags {flags:x} {left:x}/{right:x}"
                    );
                    let left_key = sort_key_for_locale(&[left], flags, locale).unwrap();
                    let right_key = sort_key_for_locale(&[right], flags, locale).unwrap();
                    assert_eq!(left_key.cmp(&right_key), ordering);
                }
            }
        }
    }
    assert_eq!(
        compare_for_locale(&[0x69], &[0x130], 1, 0x41f).unwrap(),
        Ordering::Less
    );
    assert_eq!(
        compare_for_locale(&[0x131], &[0x49], 1, 0x41f).unwrap(),
        Ordering::Greater
    );
}
#[test]
fn filesystem_casing_adapter_preserves_unrelated_turkish_tailoring() {
    for left in [
        "c", "ç", "d", "g", "ğ", "h", "o", "ö", "p", "s", "ş", "t", "u", "ü", "v",
    ] {
        for right in [
            "c", "ç", "d", "g", "ğ", "h", "o", "ö", "p", "s", "ş", "t", "u", "ü", "v",
        ] {
            let left = left.encode_utf16().collect::<Vec<_>>();
            let right = right.encode_utf16().collect::<Vec<_>>();
            for flags in [0, 1, 0x10, 2, 3, 0x1000] {
                assert_eq!(
                    compare_for_locale(&left, &right, flags, 0x41f).unwrap(),
                    compare_for_locale(&left, &right, flags | 0x08000000, 0x41f).unwrap()
                );
            }
        }
    }
}
