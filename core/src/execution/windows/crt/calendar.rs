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

pub(super) fn strftime(memory: &mut GuestMemory, args: &[u32]) -> Result<u32, DispatchError> {
    let [output, maxsize, format, time] = [args[0], args[1], args[2], args[3]];
    if output == 0 || format == 0 || time == 0 || maxsize > 256 {
        return Err(DispatchError::Unsupported);
    }
    let format = read_format(memory, format)?;
    let mut fields = [0; 9];
    guest::read_words(memory, time, &mut fields)?;
    if !valid_time(&fields) {
        return Err(DispatchError::Unsupported);
    }
    let weekdays: [&[u8]; 7] = [b"Sun", b"Mon", b"Tue", b"Wed", b"Thu", b"Fri", b"Sat"];
    let months: [&[u8]; 12] = [
        b"Jan", b"Feb", b"Mar", b"Apr", b"May", b"Jun", b"Jul", b"Aug", b"Sep", b"Oct", b"Nov",
        b"Dec",
    ];
    let mut result = Vec::new();
    let mut chars = format.into_iter();
    while let Some(byte) = chars.next() {
        match byte {
            b'%' => match chars.next().ok_or(DispatchError::Unsupported)? {
                b'a' => result.extend_from_slice(weekdays[fields[6] as usize]),
                b'b' => result.extend_from_slice(months[fields[4] as usize]),
                b'd' => append_two(&mut result, fields[3]),
                b'H' => append_two(&mut result, fields[2]),
                b'M' => append_two(&mut result, fields[1]),
                _ => return Err(DispatchError::Unsupported),
            },
            b' ' | b':' => result.push(byte),
            _ => return Err(DispatchError::Unsupported),
        }
        if result.len() > 256 {
            return Ok(0);
        }
    }
    if result.len() + 1 > maxsize as usize {
        return Ok(0);
    }
    let count = u32::try_from(result.len()).expect("bounded date output");
    result.push(0);
    guest::check(memory, output, result.len(), Access::Write)?;
    memory.write(u64::from(output), &result)?;
    Ok(count)
}

fn read_format(memory: &GuestMemory, address: u32) -> Result<Vec<u8>, DispatchError> {
    let mut result = Vec::new();
    for offset in 0..256 {
        let position = address
            .checked_add(offset)
            .ok_or(MemoryError::AddressOverflow)?;
        let mut byte = [0];
        memory.read(u64::from(position), &mut byte)?;
        if byte[0] == 0 {
            return Ok(result);
        }
        result.push(byte[0]);
    }
    Err(DispatchError::Unsupported)
}

fn valid_time(fields: &[u32; 9]) -> bool {
    let month = fields[4];
    let Some(year) = 1900_u32.checked_add(fields[5]) else {
        return false;
    };
    let days = [
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
    fields[0] <= 59
        && fields[1] <= 59
        && fields[2] <= 23
        && month < 12
        && (1..=days[month as usize]).contains(&fields[3])
        && fields[6] <= 6
        && fields[7] < if leap_year(year) { 366 } else { 365 }
        && matches!(fields[8], 0 | 1 | u32::MAX)
}

fn append_two(result: &mut Vec<u8>, value: u32) {
    result.push(b'0' + u8::try_from(value / 10).expect("validated two-digit value"));
    result.push(b'0' + u8::try_from(value % 10).expect("validated two-digit value"));
}
