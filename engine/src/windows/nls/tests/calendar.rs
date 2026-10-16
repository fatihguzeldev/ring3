use crate::windows::nls::{NlsError32, calendar::*};
#[test]
fn gregorian_filetime_epoch_leap_days_and_literal_systemtime_layout_round_trip() {
    assert_eq!(
        from_filetime(0).unwrap(),
        SystemTime32 {
            year: 1601,
            month: 1,
            day: 1,
            day_of_week: 1,
            ..Default::default()
        }
    );
    let epoch = SystemTime32 {
        year: 1970,
        month: 1,
        day: 1,
        day_of_week: 6,
        ..Default::default()
    };
    assert_eq!(to_filetime(epoch).unwrap(), 116444736000000000);
    assert_eq!(
        from_filetime(to_filetime(epoch).unwrap())
            .unwrap()
            .day_of_week,
        4
    );
    for year in [1601, 1900, 2000, 2024, 2100, 2400, 30827] {
        let value = SystemTime32 {
            year,
            month: 12,
            day: 31,
            hour: 23,
            minute: 59,
            second: 59,
            millisecond: 999,
            ..Default::default()
        };
        let back = from_filetime(to_filetime(value).unwrap()).unwrap();
        assert_eq!(
            (
                back.year,
                back.month,
                back.day,
                back.hour,
                back.minute,
                back.second,
                back.millisecond
            ),
            (year, 12, 31, 23, 59, 59, 999)
        );
        assert_eq!(SystemTime32::from_bytes(back.bytes()), back);
    }
    assert_eq!(
        to_filetime(SystemTime32 {
            year: 1900,
            month: 2,
            day: 29,
            ..Default::default()
        }),
        Err(NlsError32::InvalidParameter)
    );
    assert!(
        to_filetime(SystemTime32 {
            year: 2000,
            month: 2,
            day: 29,
            ..Default::default()
        })
        .is_ok()
    );
    assert!(from_filetime(1 << 63).is_err());
}
#[test]
fn authored_date_time_tokens_quotes_and_bias_are_deterministic() {
    let time = SystemTime32 {
        year: 2024,
        month: 2,
        day: 29,
        hour: 13,
        minute: 5,
        second: 7,
        ..Default::default()
    };
    assert_eq!(
        format_date(time, "dddd, MMMM d, yyyy").unwrap(),
        "Thursday, February 29, 2024"
    );
    assert_eq!(
        format_time(time, "h:mm:ss tt 'quoted'").unwrap(),
        "1:05:07 PM quoted"
    );
    assert_eq!(
        format_date(time, "yyyy 'unterminated"),
        Err(NlsError32::InvalidParameter)
    );
    assert_eq!(shift_filetime(0, -1), Err(NlsError32::InvalidParameter));
}
