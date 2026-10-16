use super::*;
use crate::windows::{
    nls::{NlsError32, NlsProfile32},
    text::CodePage32,
};
fn text(engine: &EngineInstance, address: u32, length: u32) -> String {
    let mut bytes = vec![0; length as usize * 2];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    let units: Vec<_> = bytes
        .chunks_exact(2)
        .map(|value| u16::from_le_bytes([value[0], value[1]]))
        .take_while(|&value| value != 0)
        .collect();
    String::from_utf16(&units).unwrap()
}
#[test]
fn turkish_casing_thread_acp_and_distinct_process_profile_use_real_locale_data() {
    let mut engine = engine();
    engine
        .configure_nls(NlsProfile32 {
            locale: 0x41f,
            ansi: CodePage32::from_id(1254).unwrap(),
            oem: CodePage32::from_id(857).unwrap(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(call(&mut engine, NlsApi32::GetACP, &[]), 1254);
    assert_eq!(call(&mut engine, NlsApi32::GetOEMCP, &[]), 857);
    wide(&mut engine, 0x10000, "iıIİ");
    let count = call(
        &mut engine,
        NlsApi32::LCMapStringW,
        &[0x41f, 0x01000200, 0x10000, -1i32 as u32, 0x11000, 64],
    );
    assert_eq!(text(&engine, 0x11000, count), "İIIİ");
    let count = call(
        &mut engine,
        NlsApi32::LCMapStringW,
        &[0x41f, 0x01000100, 0x10000, -1i32 as u32, 0x11000, 64],
    );
    assert_eq!(text(&engine, 0x11000, count), "iııi");
    assert_eq!(call(&mut engine, NlsApi32::SetThreadLocale, &[0x411]), 1);
    write(&mut engine, 0x10000, &[0x82, 0xa0]);
    assert_eq!(
        call(
            &mut engine,
            NlsApi32::MultiByteToWideChar,
            &[3, 8, 0x10000, 2, 0x11000, 8]
        ),
        1
    );
    assert_eq!(read::<2>(&engine, 0x11000), [0x42, 0x30]);
    assert_eq!(call(&mut engine, NlsApi32::GetACP, &[]), 1254);
    write(&mut engine, 0x10000, &[0xfd]);
    assert_eq!(
        call(
            &mut engine,
            NlsApi32::MultiByteToWideChar,
            &[0, 8, 0x10000, 1, 0x11000, 8]
        ),
        1
    );
    assert_eq!(read::<2>(&engine, 0x11000), [0x31, 0x01]);
}
#[test]
fn french_months_german_numeric_metadata_and_calendar_patterns_are_not_english_fallbacks() {
    let mut engine = engine();
    let count = call(
        &mut engine,
        NlsApi32::GetLocaleInfoW,
        &[0x40c, 0x39, 0x11000, 64],
    );
    assert_eq!(text(&engine, 0x11000, count), "février");
    let count = call(
        &mut engine,
        NlsApi32::GetLocaleInfoW,
        &[0x407, 0xe, 0x11000, 64],
    );
    assert_eq!(text(&engine, 0x11000, count), ",");
    let count = call(
        &mut engine,
        NlsApi32::GetLocaleInfoW,
        &[0x407, 0xf, 0x11000, 64],
    );
    assert_eq!(text(&engine, 0x11000, count), ".");
    let count = call(
        &mut engine,
        NlsApi32::GetLocaleInfoW,
        &[0x41f, 5, 0x11000, 64],
    );
    assert_eq!(text(&engine, 0x11000, count), "90");
    write(
        &mut engine,
        0x10000,
        &crate::windows::nls::calendar::SystemTime32 {
            year: 2024,
            month: 2,
            day: 29,
            ..Default::default()
        }
        .bytes(),
    );
    wide(&mut engine, 0x10040, "dddd, d. MMMM yyyy");
    let count = call(
        &mut engine,
        NlsApi32::GetDateFormatW,
        &[0x407, 0, 0x10000, 0x10040, 0x11000, 128],
    );
    assert_eq!(
        text(&engine, 0x11000, count),
        "Donnerstag, 29. Februar 2024"
    );
    write(&mut engine, 0x11000, &[0xa5; 8]);
    assert_eq!(
        engine.execute_nls_call(NlsApi32::GetLocaleInfoW, &[0xf00411, 0xe, 0x11000, 64]),
        Err(NlsError32::InvalidParameter)
    );
    assert_eq!(read::<8>(&engine, 0x11000), [0xa5; 8]);
}

#[test]
fn russian_standalone_and_requested_genitive_months_preserve_context() {
    let mut engine = engine();
    let length = call(
        &mut engine,
        NlsApi32::GetLocaleInfoW,
        &[0x419, 0x38, 0x11000, 64],
    );
    assert_eq!(text(&engine, 0x11000, length), "январь");
    let length = call(
        &mut engine,
        NlsApi32::GetLocaleInfoW,
        &[0x419, 0x10000038, 0x11000, 64],
    );
    assert_eq!(text(&engine, 0x11000, length), "января");
    write(
        &mut engine,
        0x10000,
        &crate::windows::nls::calendar::SystemTime32 {
            year: 2024,
            month: 1,
            day: 2,
            ..Default::default()
        }
        .bytes(),
    );
    wide(&mut engine, 0x10040, "d MMMM yyyy");
    let length = call(
        &mut engine,
        NlsApi32::GetDateFormatW,
        &[0x419, 0, 0x10000, 0x10040, 0x11000, 64],
    );
    assert_eq!(text(&engine, 0x11000, length), "2 января 2024");
}

#[test]
fn month_day_and_alternate_calendar_select_their_own_pictures() {
    let mut engine = engine();
    write(
        &mut engine,
        0x10000,
        &crate::windows::nls::calendar::SystemTime32 {
            year: 2024,
            month: 2,
            day: 29,
            ..Default::default()
        }
        .bytes(),
    );
    let length = call(
        &mut engine,
        NlsApi32::GetDateFormatW,
        &[0x409, 0x80, 0x10000, 0, 0x11000, 64],
    );
    assert_eq!(text(&engine, 0x11000, length), "February 29");
    let length = call(
        &mut engine,
        NlsApi32::GetLocaleInfoW,
        &[0x409, 0x78, 0x11000, 64],
    );
    assert_eq!(text(&engine, 0x11000, length), "MMMM d");
    let length = call(
        &mut engine,
        NlsApi32::GetCalendarInfoW,
        &[0x409, 1, 0x38, 0x11000, 64, 0],
    );
    assert_eq!(text(&engine, 0x11000, length), "MMMM d");
    let length = call(
        &mut engine,
        NlsApi32::GetDateFormatW,
        &[0x411, 4, 0x10000, 0, 0x11000, 64],
    );
    assert!(text(&engine, 0x11000, length).contains("令和"));
    wide(&mut engine, 0x10040, "yyyy-MM-dd");
    let length = call(
        &mut engine,
        NlsApi32::GetDateFormatW,
        &[0x411, 4, 0x10000, 0x10040, 0x11000, 64],
    );
    assert_eq!(text(&engine, 0x11000, length), "0006-02-29");
    assert_eq!(
        engine.execute_nls_call(
            NlsApi32::GetDateFormatW,
            &[0x411, 0x80000004, 0x10000, 0x10040, 0x11000, 64],
        ),
        Err(NlsError32::InvalidFlags)
    );
}
