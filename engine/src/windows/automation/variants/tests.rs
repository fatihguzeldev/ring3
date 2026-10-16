mod native;
use super::*;
use crate::windows::automation::AutomationError32 as E;
fn numeric(value: f64) -> Number32 {
    Variant32::new(5, &value.to_bits().to_le_bytes())
        .number()
        .unwrap()
}
#[test]
fn native_bankers_rounding_and_integer_boundaries_use_the_shared_extended_arithmetic() {
    for (input, expected) in [
        (2.5, 2i32),
        (3.5, 4),
        (-2.5, -2),
        (-3.5, -4),
        (-2147483648.5, i32::MIN),
    ] {
        assert_eq!(
            &numeric(input).convert(3).unwrap().0[8..12],
            &expected.to_le_bytes()
        );
    }
    assert_eq!(numeric(2147483647.5).convert(3), Err(E::Overflow));
    assert_eq!(
        Number32::Integer(i128::from(u64::MAX))
            .convert(21)
            .unwrap()
            .0[8..],
        u64::MAX.to_le_bytes()
    );
    assert_eq!(
        Number32::Integer(i128::from(u64::MAX) + 1).convert(21),
        Err(E::Overflow)
    );
    assert_eq!(numeric(f64::INFINITY).convert(3), Err(E::Overflow));
}
#[test]
fn native_locale_decimal_and_group_separators_preserve_exact_unsigned_values() {
    let german = NumberSymbols32 {
        decimal: ",".into(),
        group: ".".into(),
        negative: "-".into(),
        currency: "€".into(),
    };
    let english = NumberSymbols32 {
        decimal: ".".into(),
        group: ",".into(),
        negative: "-".into(),
        currency: "$".into(),
    };
    for (text, symbols, expected) in [
        ("1,5", &german, 1.5f64),
        ("1.234,5", &german, 1234.5),
        ("1,5", &english, 15.0),
    ] {
        let result = Number32::parse(&text.encode_utf16().collect::<Vec<_>>(), symbols)
            .unwrap()
            .convert(5)
            .unwrap();
        assert_eq!(&result.0[8..], &expected.to_bits().to_le_bytes());
    }
    let maximum = Number32::parse(
        &"18446744073709551615".encode_utf16().collect::<Vec<_>>(),
        &english,
    )
    .unwrap();
    assert_eq!(
        &maximum.convert(21).unwrap().0[8..],
        &u64::MAX.to_le_bytes()
    );
    let overflow = Number32::parse(
        &"18446744073709551616".encode_utf16().collect::<Vec<_>>(),
        &english,
    )
    .unwrap();
    assert_eq!(overflow.convert(21), Err(E::Overflow));
}
#[test]
fn decimal_96_bit_layout_and_rounding_do_not_pass_through_host_floating_point() {
    let decimal = Decimal32 {
        coefficient: Decimal32::MAX,
        negative: false,
        scale: 28,
    };
    assert_eq!(
        Decimal32::decode(decimal.encode().unwrap()).unwrap(),
        decimal
    );
    assert_eq!(decimal.rounded().unwrap(), 8);
    assert_eq!(
        Decimal32 {
            coefficient: 25,
            negative: true,
            scale: 1
        }
        .rounded()
        .unwrap(),
        -2
    );
    assert_eq!(
        Decimal32 {
            coefficient: 35,
            negative: false,
            scale: 1
        }
        .rounded()
        .unwrap(),
        4
    );
    assert_eq!(
        Decimal32 {
            coefficient: 123400,
            negative: false,
            scale: 4
        }
        .text(),
        "12.34"
    );
}
#[test]
fn variant_wire_types_and_native_clear_preserve_only_the_unused_payload() {
    let value = Variant32::new(3, &7i32.to_le_bytes());
    assert_eq!(value.cleared().vartype(), 0);
    assert_eq!(&value.cleared().0[8..12], &7i32.to_le_bytes());
    assert_eq!(
        Variant32::new(0x4000, &0u32.to_le_bytes()).validate(),
        Err(E::BadVariantType)
    );
    assert_eq!(Variant32::new(12, &[]).validate(), Err(E::BadVariantType));
    assert_eq!(
        Variant32::new(0x400c, &1u32.to_le_bytes()).value_bytes(),
        16
    );
}
