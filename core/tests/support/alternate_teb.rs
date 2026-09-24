use ring3_core::execution::{PAGE_SIZE, Permissions, Process32};

pub fn map(process: &mut Process32, base: u32, id: u32) {
    process
        .memory
        .map_zeroed(u64::from(base), PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    for (offset, value) in [
        (0, u32::MAX),
        (4, 0x1001_0000),
        (8, 0x1000_0000),
        (0x18, base),
        (0x24, id),
    ] {
        process
            .memory
            .write(u64::from(base + offset), &value.to_le_bytes())
            .unwrap();
    }
}
