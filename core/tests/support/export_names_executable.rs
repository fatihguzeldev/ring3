use super::dll_executable::{attach, dll as image, put};

pub fn dll() -> Vec<u8> {
    let base = 0x5000_0000;
    let mut bytes = image(base, &attach(base, 41, true), None);
    bytes.resize(0x26400, 0);
    for (at, value) in [
        (0xd0, 0x28000),
        (0x1a8, 0x26000),
        (0x1b0, 0x26000),
        (1560, 2050),
        (1568, 0x3000),
        (1572, 0x6000),
    ] {
        put(&mut bytes, at, value);
    }
    let offset = |rva: usize| rva - 0x2000 + 1024;
    put(&mut bytes, offset(0x3000), 0x2270);
    bytes[offset(0x6000)..offset(0x6000) + 2].copy_from_slice(&1_u16.to_le_bytes());
    for index in 0..2048 {
        let rva = 0x8000 + index * 64;
        put(
            &mut bytes,
            offset(0x3000 + (index + 1) * 4),
            u32::try_from(rva).unwrap(),
        );
        let name = format!("symbol_{index:04}");
        let start = offset(rva);
        bytes[start..start + 63].fill(b'_');
        bytes[start..start + name.len()].copy_from_slice(name.as_bytes());
    }
    put(&mut bytes, offset(0x3000 + 2049 * 4), 0x2260);
    bytes
}
