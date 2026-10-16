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
        format_date(time, "yyyy 'unterminated").unwrap(),
        "2024 unterminated"
    );
    assert_eq!(shift_filetime(0, -1), Err(NlsError32::InvalidParameter));
}

#[test]
fn thai_japanese_and_hebrew_calendar_operations_use_portable_calendar_arithmetic() {
    let time = SystemTime32 {
        year: 2024,
        month: 2,
        day: 29,
        ..Default::default()
    };
    assert_eq!(
        format_calendar_date(time, "yyyy-MM-dd", 0x41e, 7).unwrap(),
        "2567-02-29"
    );
    assert_eq!(
        format_calendar_date(time, "gg y年M月d日", 0x411, 3).unwrap(),
        "令和 6年2月29日"
    );
    let (month, _) = calendar_text(0x40d, 8, 33).unwrap();
    assert_eq!(month, "אלול");
    assert!(format_calendar_date(time, "yyyy", 0x409, 0xffff).is_err());
}

#[test]
fn date_format_ignores_all_time_fields_and_recovers_best_possible_quoted_picture() {
    let time = SystemTime32 {
        year: 2024,
        month: 2,
        day: 29,
        hour: 999,
        minute: 999,
        second: 999,
        millisecond: 65535,
        ..Default::default()
    };
    assert_eq!(
        format_date(time, "dddd, MMMM d, yyyy").unwrap(),
        "Thursday, February 29, 2024"
    );
    assert_eq!(format_date(time, "MMMM ''''yy").unwrap(), "February '24");
    assert_eq!(format_date(time, "yyyy 'literal").unwrap(), "2024 literal");
}

#[test]
fn gregorian_language_variants_share_date_math_but_keep_distinct_month_and_weekday_names() {
    let time = SystemTime32 {
        year: 2011,
        month: 8,
        day: 7,
        ..Default::default()
    };
    for (calendar, month, weekday) in [
        (9, "août", "dimanche"),
        (10, "آب", "الأحد"),
        (11, "أغسطس", "الأحد"),
        (12, "أوت", "الأحد"),
    ] {
        assert_eq!(
            format_calendar_date(time, "yyyy-MM-dd", 0x401, calendar).unwrap(),
            "2011-08-07"
        );
        assert_eq!(
            format_calendar_date(time, "MMMM", 0x401, calendar).unwrap(),
            month
        );
        assert_eq!(
            format_calendar_date(time, "dddd", 0x401, calendar).unwrap(),
            weekday
        );
        assert_eq!(calendar_text(0x401, calendar, 28).unwrap().0, month);
        assert_eq!(calendar_text(0x401, calendar, 1).unwrap().1, Some(calendar));
    }
    assert_eq!(CalendarId32::from_id(13), Err(NlsError32::InvalidParameter));
}
