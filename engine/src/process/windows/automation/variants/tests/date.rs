use super::*;

#[test]
fn captured_native_date_text_and_parsing_use_locale_order_and_absolute_negative_fractions() {
    let mut engine = initialized();
    let dates = [0.0f64, 1.5, -1.25, 36925.0, 36925.54309027778];
    let english = [
        "12:00:00 AM",
        "12/31/1899 12:00:00 PM",
        "12/29/1899 6:00:00 AM",
        "2/3/2001",
        "2/3/2001 1:02:03 PM",
    ];
    let german = [
        "00:00:00",
        "31.12.1899 12:00:00",
        "29.12.1899 06:00:00",
        "03.02.2001",
        "03.02.2001 13:02:03",
    ];
    // windows x86 run 38967957696, source 07c60bbb: only the active
    // date member and counted bstr payload are observable oracles here.
    for (locale, expected) in [(0x409, english), (0x407, german)] {
        for (date, expected) in dates.into_iter().zip(expected) {
            put(
                &mut engine,
                0x1400,
                Variant32::new(7, &date.to_bits().to_le_bytes()),
            );
            put(&mut engine, 0x1500, Variant32::EMPTY);
            assert_eq!(
                call(
                    &mut engine,
                    Api::VariantChangeTypeEx,
                    &[0x1500, 0x1400, locale, 0, 8]
                ),
                0,
                "{locale:x}/{date}"
            );
            let pointer = value(&engine, 0x1500).pointer();
            assert_eq!(
                engine.read_automation_bstr(pointer).unwrap(),
                expected
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>()
            );
            assert_eq!(call(&mut engine, Api::VariantClear, &[0x1500]), 0);
        }
    }
    let inputs = [
        "2/3/2001",
        "03.02.2001",
        "2001-02-03",
        "1:02:03 PM",
        "12:00 AM",
        "invalid",
    ];
    for (locale, expected) in [
        (
            0x409,
            [
                Some(36925.0f64),
                None,
                Some(36925.0),
                Some(0.5430902777777777),
                Some(0.0),
                None,
            ],
        ),
        (
            0x407,
            [
                Some(36952.0f64),
                Some(36925.0),
                Some(36925.0),
                Some(0.5430902777777777),
                Some(0.0),
                None,
            ],
        ),
    ] {
        for (input, expected) in inputs.into_iter().zip(expected) {
            let pointer = text(&mut engine, 0x1100, input);
            put(
                &mut engine,
                0x1400,
                Variant32::new(8, &pointer.to_le_bytes()),
            );
            let old = Variant32::new(3, &37i32.to_le_bytes());
            put(&mut engine, 0x1500, old);
            let result = call(
                &mut engine,
                Api::VariantChangeTypeEx,
                &[0x1500, 0x1400, locale, 0, 7],
            );
            if let Some(expected) = expected {
                assert_eq!(result, 0, "{locale:x}/{input}");
                assert_eq!(
                    value(&engine, 0x1500),
                    Variant32::new(7, &expected.to_bits().to_le_bytes()),
                    "{locale:x}/{input}"
                );
            } else {
                assert_eq!(result, 0x8002_0005, "{locale:x}/{input}");
                assert_eq!(value(&engine, 0x1500), old);
            }
            assert_eq!(call(&mut engine, Api::VariantClear, &[0x1400]), 0);
        }
    }
    assert_eq!(engine.automation.bytes, 0);
}

#[test]
fn captured_native_empty_to_bstr_allocates_empty_while_null_rejects_non_null_targets() {
    let mut engine = initialized();
    for source in [0, 1] {
        for target in [0, 1, 3, 5, 11, 8, 7, 14, 13] {
            put(&mut engine, 0x1400, Variant32::new(source, &[]));
            let old = Variant32::new(3, &37i32.to_le_bytes());
            put(&mut engine, 0x1500, old);
            let result = call(
                &mut engine,
                Api::VariantChangeTypeEx,
                &[0x1500, 0x1400, 0x409, 0, target],
            );
            if source == 1 && target != 1 || source == 0 && target == 13 {
                assert_eq!(result, 0x8002_0005, "{source}/{target}");
                assert_eq!(value(&engine, 0x1500), old);
            } else {
                assert_eq!(result, 0, "{source}/{target}");
                let value = value(&engine, 0x1500);
                assert_eq!(value.vartype(), target as u16);
                if target == 8 {
                    assert_ne!(value.pointer(), 0);
                    assert!(
                        engine
                            .read_automation_bstr(value.pointer())
                            .unwrap()
                            .is_empty()
                    );
                } else {
                    assert!(value.0[8..].iter().all(|byte| *byte == 0));
                }
                assert_eq!(call(&mut engine, Api::VariantClear, &[0x1500]), 0);
            }
        }
    }
    assert_eq!(engine.automation.bytes, 0);
}
