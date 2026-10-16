use super::*;
use crate::windows::nls::calendar::{SystemTime32, to_filetime};
#[test]
fn filetime_systemtime_dos_and_timezone_outputs_share_one_virtual_epoch() {
    let mut engine = engine();
    let time = SystemTime32 {
        year: 2024,
        month: 2,
        day: 29,
        hour: 13,
        minute: 5,
        second: 8,
        ..Default::default()
    };
    write(&mut engine, 0x10000, &time.bytes());
    assert_eq!(
        call(
            &mut engine,
            NlsApi32::SystemTimeToFileTime,
            &[0x10000, 0x11000]
        ),
        1
    );
    assert_eq!(
        u64::from_le_bytes(read(&engine, 0x11000)),
        to_filetime(time).unwrap()
    );
    assert_eq!(
        call(
            &mut engine,
            NlsApi32::FileTimeToDosDateTime,
            &[0x11000, 0x12000, 0x12002]
        ),
        1
    );
    let date = u16::from_le_bytes(read(&engine, 0x12000));
    let clock = u16::from_le_bytes(read(&engine, 0x12002));
    assert_eq!(
        call(
            &mut engine,
            NlsApi32::DosDateTimeToFileTime,
            &[u32::from(date), u32::from(clock), 0x12008]
        ),
        1
    );
    assert_eq!(
        u64::from_le_bytes(read(&engine, 0x12008)),
        to_filetime(time).unwrap()
    );
}
