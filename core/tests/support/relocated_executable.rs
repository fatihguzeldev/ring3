use super::dll_executable::{attach, dll as image, put};

pub fn dll(base: u32) -> Vec<u8> {
    let mut bytes = image(base, &attach(base, 41, true), None);
    put(&mut bytes, 0x120, 0x2300);
    put(&mut bytes, 0x124, 20);
    put(&mut bytes, 0x700, 0x1000);
    put(&mut bytes, 0x704, 20);
    for (index, entry) in [0x3005_u16, 0x300e, 0x3017, 0x301d, 0x3081, 0]
        .into_iter()
        .enumerate()
    {
        bytes[0x708 + index * 2..0x70a + index * 2].copy_from_slice(&entry.to_le_bytes());
    }
    bytes
}
