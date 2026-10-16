use super::*;
#[test]
fn locale_profile_numeric_output_and_unicode_character_masks_are_not_host_dependent() {
    let mut engine = engine();
    assert_eq!(call(&mut engine, NlsApi32::GetACP, &[]), 1252);
    assert_eq!(call(&mut engine, NlsApi32::GetOEMCP, &[]), 437);
    assert_eq!(
        call(
            &mut engine,
            NlsApi32::GetLocaleInfoW,
            &[0x400, 0x20001004, 0x11000, 2]
        ),
        2
    );
    assert_eq!(u32::from_le_bytes(read(&engine, 0x11000)), 1252);
    wide(&mut engine, 0x10000, "Éα中");
    assert_eq!(
        call(
            &mut engine,
            NlsApi32::GetStringTypeW,
            &[1, 0x10000, 3, 0x11000]
        ),
        1
    );
    assert_ne!(u16::from_le_bytes(read(&engine, 0x11000)) & 0x100, 0);
    assert_ne!(u16::from_le_bytes(read(&engine, 0x11004)) & 0x100, 0);
}

#[test]
fn numeric_calendar_pointer_combinations_fail_before_output_publication() {
    let mut engine = engine();
    write(&mut engine, 0x11000, &[0xa5; 8]);
    assert!(
        engine
            .execute_nls_call(
                NlsApi32::GetCalendarInfoW,
                &[0x409, 1, 0x20000030, 0x11004, 0, 0x11000]
            )
            .is_err()
    );
    assert_eq!(read::<8>(&engine, 0x11000), [0xa5; 8]);
    assert_eq!(
        call(
            &mut engine,
            NlsApi32::GetCalendarInfoW,
            &[0x409, 1, 0x20000030, 0, 0, 0x11000]
        ),
        2
    );
    assert_eq!(u32::from_le_bytes(read(&engine, 0x11000)), 2029);
}

#[test]
fn calendar_constants_are_literal_windows_abi_values_not_adjacent_aliases() {
    let mut engine = engine();
    write(&mut engine, 0x11000, &[0xa5; 8]);
    assert_eq!(
        engine.execute_nls_call(
            NlsApi32::GetCalendarInfoW,
            &[0x409, 1, 0x20000031, 0, 0, 0x11000]
        ),
        Err(crate::windows::nls::NlsError32::InvalidFlags)
    );
    assert_eq!(read::<8>(&engine, 0x11000), [0xa5; 8]);
    let length = call(
        &mut engine,
        NlsApi32::GetCalendarInfoW,
        &[0x409, 1, 0x2f, 0x11000, 32, 0],
    );
    let mut bytes = vec![0; length as usize * 2];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x11000), &mut bytes)
        .unwrap();
    assert_eq!(
        bytes,
        "MMMM yyyy\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        call(
            &mut engine,
            NlsApi32::GetLocaleInfoW,
            &[0x409, 0x20001009, 0x11000, 2]
        ),
        2
    );
    assert_eq!(u32::from_le_bytes(read(&engine, 0x11000)), 1);
    assert_eq!(
        call(
            &mut engine,
            NlsApi32::GetLocaleInfoW,
            &[0x409, 0x100e, 0x11000, 2]
        ),
        1
    );
    assert_eq!(read::<2>(&engine, 0x11000), [0, 0]);
}
