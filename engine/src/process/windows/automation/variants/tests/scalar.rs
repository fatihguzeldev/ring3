use super::*;

#[test]
fn captured_variant_initialization_clear_and_bankers_rounding_match_native_wire_results() {
    let mut engine = initialized();
    put(&mut engine, 0x1500, Variant32([0xa5; 16]));
    call(&mut engine, Api::VariantInit, &[0x1500]);
    assert_eq!(value(&engine, 0x1500), Variant32::EMPTY);
    for (input, expected) in [
        (2.5f64, 2i32),
        (3.5, 4),
        (-2.5, -2),
        (-3.5, -4),
        (-2147483648.5, i32::MIN),
    ] {
        put(
            &mut engine,
            0x1400,
            Variant32::new(5, &input.to_bits().to_le_bytes()),
        );
        assert_eq!(
            call(
                &mut engine,
                Api::VariantChangeTypeEx,
                &[0x1500, 0x1400, 0x409, 0, 3]
            ),
            0
        );
        assert_eq!(
            value(&engine, 0x1500),
            Variant32::new(3, &expected.to_le_bytes())
        );
    }
    let old = value(&engine, 0x1500);
    put(
        &mut engine,
        0x1400,
        Variant32::new(5, &2147483647.5f64.to_bits().to_le_bytes()),
    );
    assert_eq!(
        call(
            &mut engine,
            Api::VariantChangeTypeEx,
            &[0x1500, 0x1400, 0x409, 0, 3]
        ),
        0x8002_000a
    );
    assert_eq!(value(&engine, 0x1500), old);
    put(&mut engine, 0x1500, Variant32::new(3, &7i32.to_le_bytes()));
    assert_eq!(call(&mut engine, Api::VariantClear, &[0x1500]), 0);
    assert_eq!(value(&engine, 0x1500).vartype(), 0);
    assert_eq!(&value(&engine, 0x1500).0[8..12], &7i32.to_le_bytes());
}

#[test]
fn captured_variant_locale_decimal_ui8_and_null_distinctions_use_the_canonical_nls_profile() {
    let mut engine = initialized();
    engine
        .configure_nls(crate::windows::nls::NlsProfile32 {
            locale: 0x407,
            ..Default::default()
        })
        .unwrap();
    put(&mut engine, 0x1500, Variant32::EMPTY);
    for (input, locale, expected) in [
        ("1,5", 0x407, 1.5f64),
        ("1.234,5", 0x407, 1234.5),
        ("1,5", 0x409, 15.0),
    ] {
        let pointer = text(&mut engine, 0x1100, input);
        put(
            &mut engine,
            0x1400,
            Variant32::new(8, &pointer.to_le_bytes()),
        );
        assert_eq!(
            call(
                &mut engine,
                Api::VariantChangeTypeEx,
                &[0x1500, 0x1400, locale, 0, 5]
            ),
            0
        );
        assert_eq!(
            value(&engine, 0x1500),
            Variant32::new(5, &expected.to_bits().to_le_bytes())
        );
        assert_eq!(call(&mut engine, Api::VariantClear, &[0x1400]), 0);
    }
    let pointer = text(&mut engine, 0x1100, "18446744073709551615");
    put(
        &mut engine,
        0x1400,
        Variant32::new(8, &pointer.to_le_bytes()),
    );
    assert_eq!(
        call(
            &mut engine,
            Api::VariantChangeTypeEx,
            &[0x1500, 0x1400, 0x409, 0, 21]
        ),
        0
    );
    assert_eq!(
        value(&engine, 0x1500),
        Variant32::new(21, &u64::MAX.to_le_bytes())
    );
    call(&mut engine, Api::VariantClear, &[0x1400]);
    put(
        &mut engine,
        0x1400,
        Variant32::new(11, &(-1i16).to_le_bytes()),
    );
    assert_eq!(
        call(
            &mut engine,
            Api::VariantChangeType,
            &[0x1500, 0x1400, 0x10, 8]
        ),
        0
    );
    let pointer = value(&engine, 0x1500).pointer();
    assert_eq!(read(&engine, pointer, 8), [87, 0, 97, 0, 104, 0, 114, 0]);
    call(&mut engine, Api::VariantClear, &[0x1500]);
    put(&mut engine, 0x1400, Variant32::new(1, &[]));
    assert_eq!(
        call(
            &mut engine,
            Api::VariantChangeTypeEx,
            &[0x1500, 0x1400, 0x409, 0, 3]
        ),
        0x8002_0005
    );
    put(&mut engine, 0x1400, Variant32::EMPTY);
    assert_eq!(
        call(
            &mut engine,
            Api::VariantChangeTypeEx,
            &[0x1500, 0x1400, 0x409, 0, 3]
        ),
        0
    );
    assert_eq!(
        value(&engine, 0x1500),
        Variant32::new(3, &0i32.to_le_bytes())
    );
    assert_eq!(engine.automation.bytes, 0);
}
