use super::*;
use crate::windows::nls::NlsError32;
#[test]
fn locale_name_apis_preserve_default_invariant_neutral_and_alternate_sort_identities() {
    let mut engine = engine();
    wide(&mut engine, 0x10000, "tR-tr");
    assert_eq!(
        call(&mut engine, NlsApi32::LocaleNameToLCID, &[0x10000, 0]),
        0x41f
    );
    wide(&mut engine, 0x10000, "de-DE_phoneb");
    assert_eq!(
        call(&mut engine, NlsApi32::LocaleNameToLCID, &[0x10000, 0]),
        0x10407
    );
    assert_eq!(
        call(
            &mut engine,
            NlsApi32::LCIDToLocaleName,
            &[0x10407, 0x11000, 85, 0]
        ),
        13
    );
    assert_eq!(
        read::<26>(&engine, 0x11000),
        "de-DE_phoneb\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
            .as_slice()
    );
    wide(&mut engine, 0x10000, "en");
    assert_eq!(
        call(&mut engine, NlsApi32::LocaleNameToLCID, &[0x10000, 0]),
        0x409
    );
    assert_eq!(
        call(
            &mut engine,
            NlsApi32::LocaleNameToLCID,
            &[0x10000, 0x08000000]
        ),
        9
    );
    wide(&mut engine, 0x10000, "");
    assert_eq!(
        call(&mut engine, NlsApi32::LocaleNameToLCID, &[0x10000, 0]),
        0x7f
    );
    assert_eq!(
        call(&mut engine, NlsApi32::LocaleNameToLCID, &[0, 0]),
        0x409
    );
    assert_eq!(
        call(
            &mut engine,
            NlsApi32::GetUserDefaultLocaleName,
            &[0x11000, 85]
        ),
        6
    );
    write(&mut engine, 0x11000, &[0xa5; 16]);
    assert_eq!(
        engine.execute_nls_call(NlsApi32::LCIDToLocaleName, &[0x411, 0x11000, 2, 0]),
        Err(NlsError32::InsufficientBuffer)
    );
    assert_eq!(read::<16>(&engine, 0x11000), [0xa5; 16]);
    wide(&mut engine, 0x10000, "ja-JP");
    assert_eq!(
        call(
            &mut engine,
            NlsApi32::GetLocaleInfoEx,
            &[0x10000, 0x20001004, 0x11000, 2]
        ),
        2
    );
    assert_eq!(u32::from_le_bytes(read::<4>(&engine, 0x11000)), 932);
    wide(&mut engine, 0x10000, "unknown-locale");
    assert_eq!(
        call(&mut engine, NlsApi32::IsValidLocaleName, &[0x10000]),
        0
    );
}
