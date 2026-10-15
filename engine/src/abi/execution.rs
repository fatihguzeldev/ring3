use super::header;

pub const INSTALLATION_RECORD_SIZE: usize = 32;
pub const DISPATCHER_RECORD_SIZE: usize = 32;
pub const RESIDENT_MODULE_RECORD_SIZE: usize = 24;

pub fn encode_installation(unit_id: u64, slot: u32, output: &mut [u8; INSTALLATION_RECORD_SIZE]) {
    header::write(output, *b"R3IN");
    header::write_u32(output, 16, unit_id as u32);
    header::write_u32(output, 20, (unit_id >> 32) as u32);
    header::write_u32(output, 24, slot);
}

pub fn encode_dispatcher(
    key: u64,
    pointer: u32,
    length: u32,
    output: &mut [u8; DISPATCHER_RECORD_SIZE],
) {
    header::write(output, *b"R3DP");
    for (index, value) in [key as u32, (key >> 32) as u32, pointer, length]
        .into_iter()
        .enumerate()
    {
        header::write_u32(output, 16 + index * 4, value);
    }
}

pub fn encode_resident_module(
    unit_id: u64,
    pointer: u32,
    length: u32,
    output: &mut [u8; RESIDENT_MODULE_RECORD_SIZE],
) {
    // the original resident module record has a version/size prefix without a magic header.
    for (index, value) in [
        1,
        24,
        unit_id as u32,
        (unit_id >> 32) as u32,
        pointer,
        length,
    ]
    .into_iter()
    .enumerate()
    {
        header::write_u32(output, index * 4, value);
    }
}

#[cfg(test)]
mod tests;
