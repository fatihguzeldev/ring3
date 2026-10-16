use super::super::{NlsRuntime32, buffers};
use super::timezone::TimeZone32;
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::nls::{
        NlsApi32, NlsError32,
        calendar::{SystemTime32, format_date, format_time, from_filetime, to_filetime},
    },
};
impl NlsRuntime32 {
    pub(in crate::process::windows::nls) fn calendar(
        &self,
        memory: &mut AddressSpace,
        api: NlsApi32,
        args: &[u32],
        now: u64,
    ) -> Result<u32, NlsError32> {
        use NlsApi32::*;
        let zone = TimeZone32::fixed(self.profile.time_zone_bias_minutes);
        let read_time = |memory: &AddressSpace, address| {
            buffers::fixed(memory, address).map(SystemTime32::from_bytes)
        };
        let read_file = |memory: &AddressSpace, address| {
            buffers::fixed(memory, address).map(u64::from_le_bytes)
        };
        let (address, bytes, result) = match api {
            GetSystemTime | GetLocalTime => {
                let value = if api == GetLocalTime {
                    zone.to_local(now)?
                } else {
                    now
                };
                (args[0], from_filetime(value)?.bytes().to_vec(), 0)
            }
            SystemTimeToFileTime => (
                args[1],
                to_filetime(read_time(memory, args[0])?)?
                    .to_le_bytes()
                    .to_vec(),
                1,
            ),
            FileTimeToSystemTime => (
                args[1],
                from_filetime(read_file(memory, args[0])?)?.bytes().to_vec(),
                1,
            ),
            FileTimeToLocalFileTime | LocalFileTimeToFileTime => {
                let value = read_file(memory, args[0])?;
                let value = if api == FileTimeToLocalFileTime {
                    zone.to_local(value)?
                } else {
                    zone.to_utc(value)?
                };
                (args[1], value.to_le_bytes().to_vec(), 1)
            }
            CompareFileTime => {
                return Ok(
                    read_file(memory, args[0])?.cmp(&read_file(memory, args[1])?) as i32 as u32,
                );
            }
            GetTimeZoneInformation => (args[0], zone.bytes().to_vec(), 0),
            SystemTimeToTzSpecificLocalTime | TzSpecificLocalTimeToSystemTime => {
                let zone = if args[0] == 0 {
                    zone
                } else {
                    TimeZone32::read(buffers::fixed(memory, args[0])?)?
                };
                let value = to_filetime(read_time(memory, args[1])?)?;
                let value = if api == SystemTimeToTzSpecificLocalTime {
                    zone.to_local(value)?
                } else {
                    zone.to_utc(value)?
                };
                (args[2], from_filetime(value)?.bytes().to_vec(), 1)
            }
            DosDateTimeToFileTime => {
                let date = args[0] & 0xffff;
                let time = args[1] & 0xffff;
                let value = SystemTime32 {
                    year: ((date >> 9) + 1980) as u16,
                    month: ((date >> 5) & 15) as u16,
                    day: (date & 31) as u16,
                    hour: (time >> 11) as u16,
                    minute: ((time >> 5) & 63) as u16,
                    second: ((time & 31) * 2) as u16,
                    ..Default::default()
                };
                (args[2], to_filetime(value)?.to_le_bytes().to_vec(), 1)
            }
            FileTimeToDosDateTime => {
                let value = from_filetime(read_file(memory, args[0])?)?;
                if !(1980..=2107).contains(&value.year) {
                    return Err(NlsError32::InvalidParameter);
                }
                let date = ((value.year - 1980) << 9) | (value.month << 5) | value.day;
                let time = (value.hour << 11) | (value.minute << 5) | (value.second / 2);
                if args[1] == 0 || args[2] == 0 {
                    return Err(NlsError32::InvalidParameter);
                }
                memory.write_spans(&[
                    (GuestAddress(args[1]), &date.to_le_bytes()),
                    (GuestAddress(args[2]), &time.to_le_bytes()),
                ])?;
                return Ok(1);
            }
            GetDateFormatA | GetDateFormatW | GetTimeFormatA | GetTimeFormatW => {
                self.profile.locale(args[0])?;
                let wide = api.name().ends_with('W');
                let date = matches!(api, GetDateFormatA | GetDateFormatW);
                let flags = args[1];
                if date {
                    if flags & !0x800000ff != 0 || flags & 7 != 0 && (flags & 7).count_ones() != 1 {
                        return Err(NlsError32::InvalidFlags);
                    }
                } else if flags & !0x8000000f != 0 {
                    return Err(NlsError32::InvalidFlags);
                }
                if args[3] != 0 && flags & !0x80000000 != 0 {
                    return Err(NlsError32::InvalidFlags);
                }
                let time = if args[2] == 0 {
                    from_filetime(zone.to_local(now)?)?
                } else {
                    read_time(memory, args[2])?
                };
                let pattern = if args[3] == 0 {
                    if date {
                        if flags & 2 != 0 {
                            "dddd, MMMM d, yyyy"
                        } else if flags & 8 != 0 {
                            "MMMM yyyy"
                        } else {
                            "M/d/yyyy"
                        }
                    } else if flags & 8 != 0 {
                        "HH:mm:ss"
                    } else {
                        "h:mm:ss tt"
                    }
                    .into()
                } else {
                    let units = buffers::units(
                        memory,
                        args[3],
                        -1,
                        wide,
                        self.profile.ansi,
                        self.profile.max_text_units,
                    )?;
                    String::from_utf16(&units[..units.len() - 1])
                        .map_err(|_| NlsError32::NoTranslation)?
                };
                let mut text = if date {
                    format_date(time, &pattern)?
                } else {
                    format_time(time, &pattern)?
                };
                if !date && args[3] == 0 {
                    if flags & 1 != 0 {
                        text = if flags & 8 != 0 {
                            format!("{:02}", time.hour)
                        } else {
                            format!(
                                "{} {}",
                                if time.hour % 12 == 0 {
                                    12
                                } else {
                                    time.hour % 12
                                },
                                if time.hour < 12 { "AM" } else { "PM" }
                            )
                        };
                    } else if flags & 2 != 0 {
                        text = if flags & 8 != 0 {
                            format!("{:02}:{:02}", time.hour, time.minute)
                        } else {
                            format!(
                                "{}:{:02} {}",
                                if time.hour % 12 == 0 {
                                    12
                                } else {
                                    time.hour % 12
                                },
                                time.minute,
                                if time.hour < 12 { "AM" } else { "PM" }
                            )
                        };
                    }
                    if flags & 4 != 0 {
                        text = text.replace(" AM", "").replace(" PM", "");
                    }
                }
                let units = text.encode_utf16().chain([0]).collect::<Vec<_>>();
                let bytes = buffers::encoded(&units, wide, self.profile.ansi)?;
                return buffers::output(
                    memory,
                    args[4],
                    args[5] as i32,
                    &bytes,
                    if wide { 2 } else { 1 },
                );
            }
            _ => unreachable!(),
        };
        if address == 0 {
            return Err(NlsError32::InvalidParameter);
        }
        memory.write(GuestAddress(address), &bytes)?;
        Ok(result)
    }
}
