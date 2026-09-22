use super::imported_executable;

pub const FIRST_IMAGE: u64 = 0x0040_23d0;
pub const SECOND_IMAGE: u64 = 0x0040_26b8;
pub const GROUP: u64 = 0x0040_2f60;

pub fn put(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

pub fn guest() -> Vec<u8> {
    let mut bytes = imported_executable::pe32(
        &[
            0x6a, 7, 0x68, 0, 0, 0x40, 0, 0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x89, 0xc3, 0x6a, 7,
            0x68, 0, 0, 0x40, 0, 0xff, 0x15, 0x60, 0x20, 0x40, 0, 0xcc,
        ],
        "USER32.dll",
        &["LoadIconA"],
    );
    bytes.resize(9216, 0);
    for (offset, value) in [
        (0xd0, 0x4000),
        (0x1a8, 8192),
        (0x1b0, 8192),
        (0x108, 0x2300),
        (0x10c, 3202),
    ] {
        put(&mut bytes, offset, value);
    }
    for (offset, count) in [(0, 2_u16), (32, 2), (64, 1), (88, 1), (112, 1), (136, 1)] {
        bytes[0x700 + offset + 14..0x700 + offset + 16].copy_from_slice(&count.to_le_bytes());
    }
    for (offset, value) in [
        (16, 3),
        (20, 0x8000_0020),
        (24, 14),
        (28, 0x8000_0070),
        (48, 1),
        (52, 0x8000_0040),
        (56, 2),
        (60, 0x8000_0058),
        (80, 1033),
        (84, 160),
        (104, 1033),
        (108, 176),
        (128, 7),
        (132, 0x8000_0088),
        (152, 1033),
        (156, 192),
    ] {
        put(&mut bytes, 0x700 + offset, value);
    }
    for (record, address, size) in [
        (160, FIRST_IMAGE, 744),
        (176, SECOND_IMAGE, 2216),
        (192, GROUP, 34),
    ] {
        put(
            &mut bytes,
            0x700 + record,
            u32::try_from(address - 0x0040_0000).unwrap(),
        );
        put(&mut bytes, 0x704 + record, size);
    }
    for (address, depth, colors, xor) in
        [(FIRST_IMAGE, 4_u16, 16, 512), (SECOND_IMAGE, 8, 256, 1024)]
    {
        let offset = usize::try_from(address - 0x0040_2000 + 0x400).unwrap();
        for (field, value) in [(0, 40), (4, 32), (8, 64), (20, xor)] {
            put(&mut bytes, offset + field, value);
        }
        bytes[offset + 12..offset + 14].copy_from_slice(&1_u16.to_le_bytes());
        bytes[offset + 14..offset + 16].copy_from_slice(&depth.to_le_bytes());
        for index in 0..colors {
            bytes[offset + 40 + index * 4] = u8::try_from(index).unwrap();
        }
        bytes[offset + 40 + colors * 4] = 0x12;
        bytes[offset + 40 + colors * 4 + usize::try_from(xor).unwrap()] = 0x80;
    }
    let group = usize::try_from(GROUP - 0x0040_2000 + 0x400).unwrap();
    bytes[group..group + 6].copy_from_slice(&[0, 0, 1, 0, 2, 0]);
    for (index, colors, depth, size, id) in [(0, 16, 4_u16, 744, 1_u16), (1, 0, 8, 2216, 2)] {
        let at = group + 6 + index * 14;
        bytes[at..at + 6].copy_from_slice(&[32, 32, colors, 0, 1, 0]);
        bytes[at + 6..at + 8].copy_from_slice(&depth.to_le_bytes());
        put(&mut bytes, at + 8, size);
        bytes[at + 12..at + 14].copy_from_slice(&id.to_le_bytes());
    }
    bytes
}
