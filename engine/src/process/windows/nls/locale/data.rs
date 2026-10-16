use crate::windows::nls::NlsError32;
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];
pub(super) fn value(kind: u32) -> Result<(String, Option<u32>), NlsError32> {
    let text = match kind {
        1 => "0409",
        2 => "English (United States)",
        3 => "ENU",
        4 => "English",
        5 => "1",
        6 => "United States",
        7 => "USA",
        8 => "United States",
        9 => "0409",
        0xa => "1",
        0xb => "437",
        0xc => ";",
        0xd => "0",
        0xe => ".",
        0xf => ",",
        0x10 => "3;0",
        0x11 => "2",
        0x12 => "1",
        0x13 => "0123456789",
        0x14 => "$",
        0x15 => "USD",
        0x16 => ".",
        0x17 => ",",
        0x18 => "3;0",
        0x19 | 0x1a => "2",
        0x1b | 0x1c => "0",
        0x1d => "/",
        0x1e => ":",
        0x1f => "M/d/yyyy",
        0x20 => "dddd, MMMM d, yyyy",
        0x21..=0x27 => "0",
        0x28 => "AM",
        0x29 => "PM",
        0x50 => "",
        0x51 => "-",
        0x52 | 0x53 => "1",
        0x54..=0x57 => "0",
        0x59 => "en",
        0x5a => "US",
        0x5c => "en-US",
        0x5d => "h:mm:ss",
        0x1001 => "English",
        0x1002 => "United States",
        0x1003 => "h:mm:ss tt",
        0x1004 => "1252",
        0x1005 => "1",
        0x1006 => "MMMM yyyy",
        0x1007 => "English",
        0x1008 => "United States",
        0x1009 => "10000;0",
        0x100a => "0",
        0x100b => "1",
        0x100c => "6",
        0x100d => "0",
        0x100e => "1",
        0x100f | 0x1010 => "0",
        0x1011 => "1",
        0x1012 => "0",
        0x1013 => "437",
        0x1014 => "10000",
        0x1018 => "1",
        0x1019 => "0",
        0x101a => "en-US",
        0x101b => "0",
        0x2a..=0x30 => DAYS[(kind - 0x2a) as usize],
        0x31..=0x37 => return Ok((DAYS[(kind - 0x31) as usize][..3].into(), None)),
        0x38..=0x43 => MONTHS[(kind - 0x38) as usize],
        0x44..=0x4f => return Ok((MONTHS[(kind - 0x44) as usize][..3].into(), None)),
        _ => return Err(NlsError32::InvalidFlags),
    };
    let numeric = match kind {
        1 | 9 => u32::from_str_radix(text, 16).ok(),
        5
        | 0xa
        | 0xb
        | 0xd
        | 0x11
        | 0x12
        | 0x19..=0x1c
        | 0x21..=0x27
        | 0x52..=0x57
        | 0x1004
        | 0x1005
        | 0x100a..=0x1014
        | 0x1018
        | 0x1019
        | 0x101b => text.parse().ok(),
        _ => None,
    };
    Ok((text.into(), numeric))
}
pub(super) fn calendar(kind: u32) -> Result<(String, Option<u32>), NlsError32> {
    let value = match kind {
        1 => "1",
        2 => "Gregorian Calendar",
        3 => "0",
        4 => "A.D.",
        5 => "M/d/yyyy",
        6 => "dddd, MMMM d, yyyy",
        0x30 => "MMMM yyyy",
        0x31 => "2029",
        7..=13 => DAYS[(kind - 7) as usize],
        14..=20 => return Ok((DAYS[(kind - 14) as usize][..3].into(), None)),
        21..=32 => MONTHS[(kind - 21) as usize],
        34..=45 => return Ok((MONTHS[(kind - 34) as usize][..3].into(), None)),
        _ => return Err(NlsError32::InvalidFlags),
    };
    Ok((
        value.into(),
        if matches!(kind, 1 | 3 | 0x31) {
            value.parse().ok()
        } else {
            None
        },
    ))
}
