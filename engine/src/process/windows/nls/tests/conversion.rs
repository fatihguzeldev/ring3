use super::*;
use crate::windows::nls::NlsError32;
#[test]
fn unicode_size_queries_include_terminator_only_for_negative_input_lengths() {
    let mut engine = engine();
    write(&mut engine, 0x10000, "é😀\0".as_bytes());
    assert_eq!(
        call(
            &mut engine,
            NlsApi32::MultiByteToWideChar,
            &[65001, 8, 0x10000, u32::MAX, 0, 0]
        ),
        4
    );
    assert_eq!(
        call(
            &mut engine,
            NlsApi32::MultiByteToWideChar,
            &[65001, 8, 0x10000, 6, 0x11000, 3]
        ),
        3
    );
    assert_eq!(read::<6>(&engine, 0x11000), [0xe9, 0, 0x3d, 0xd8, 0, 0xde]);
}
#[test]
fn default_flag_and_output_publish_atomically_and_invalid_huge_input_does_not_allocate() {
    let mut engine = engine();
    wide(&mut engine, 0x10000, "Ā∞");
    write(&mut engine, 0x11000, &[0xa5; 8]);
    let before = engine.memory().unwrap().transaction_version();
    let result = engine.execute_nls_call(
        NlsApi32::WideCharToMultiByte,
        &[1252, 0x400, 0x10000, 2, 0x11000, 2, 0, 0x90000000],
    );
    assert!(matches!(result, Err(NlsError32::Memory(_))));
    assert_eq!(read::<8>(&engine, 0x11000), [0xa5; 8]);
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
    assert_eq!(
        call(
            &mut engine,
            NlsApi32::WideCharToMultiByte,
            &[1252, 0x400, 0x10000, 2, 0x11000, 2, 0, 0x11004]
        ),
        2
    );
    assert_eq!(
        read::<8>(&engine, 0x11000),
        [b'?', b'?', 0xa5, 0xa5, 1, 0, 0, 0]
    );
    assert_eq!(
        engine.execute_nls_call(
            NlsApi32::MultiByteToWideChar,
            &[1252, 0, 0x10000, 0x7fffffff, 0, 0]
        ),
        Err(NlsError32::Capacity)
    );
}

#[test]
fn bounded_copy_stops_at_requested_prefix_without_reading_a_source_terminator() {
    let mut engine = engine();
    write(&mut engine, 0x12ffe, &[b'A', 0]);
    assert_eq!(
        call(&mut engine, NlsApi32::LstrcpynW, &[0x11000, 0x12ffe, 2]),
        0x11000
    );
    assert_eq!(read::<4>(&engine, 0x11000), [b'A', 0, 0, 0]);
    assert_eq!(
        call(&mut engine, NlsApi32::LstrcpynW, &[0x11000, 0xfffffff0, 1]),
        0x11000
    );
    assert_eq!(read::<2>(&engine, 0x11000), [0, 0]);
}
