use super::super::Access;
use super::{DispatchError, GuestMemory, MemoryError, guest};

const SECONDS_PER_DAY: u32 = 86_400;
const OUTPUT_OFFSET: u32 = 0x100;

pub(super) fn localtime(
    memory: &mut GuestMemory,
    source: u32,
    teb: u32,
) -> Result<u32, DispatchError> {
    let mut input = [0];
    guest::read_words(memory, source, &mut input)?;
    let seconds = input[0].cast_signed();
    if seconds < 0 {
        return Ok(0);
    }
    let seconds = seconds.cast_unsigned();
    let days = seconds / SECONDS_PER_DAY;
    let mut remaining_days = days;
    let mut year = 1970;
    loop {
        let length = if leap_year(year) { 366 } else { 365 };
        if remaining_days < length {
            break;
        }
        remaining_days -= length;
        year += 1;
    }
    let day_of_year = remaining_days;
    let months = [
        31,
        if leap_year(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 0;
    while remaining_days >= months[month as usize] {
        remaining_days -= months[month as usize];
        month += 1;
    }
    let seconds_today = seconds % SECONDS_PER_DAY;
    let fields = [
        seconds_today % 60,
        (seconds_today / 60) % 60,
        seconds_today / 3600,
        remaining_days + 1,
        month,
        year - 1900,
        (days + 4) % 7,
        day_of_year,
        0,
    ];
    let output = teb
        .checked_add(OUTPUT_OFFSET)
        .ok_or(MemoryError::AddressOverflow)?;
    let mut bytes = [0; 36];
    for (field, chunk) in fields.into_iter().zip(bytes.chunks_exact_mut(4)) {
        chunk.copy_from_slice(&field.to_le_bytes());
    }
    guest::check(memory, output, bytes.len(), Access::Write)?;
    memory.write(u64::from(output), &bytes)?;
    Ok(output)
}

fn leap_year(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}
