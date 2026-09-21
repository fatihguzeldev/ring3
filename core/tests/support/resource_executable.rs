use super::imported_executable;

pub fn put(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

pub fn block(strings: &[&[u16]]) -> Vec<u16> {
    assert!(strings.len() <= 16);
    let mut units = Vec::new();
    for index in 0..16 {
        let string = strings.get(index).copied().unwrap_or(&[]);
        units.push(u16::try_from(string.len()).unwrap());
        units.extend_from_slice(string);
    }
    units
}

pub fn pe32(code: &[u8], languages: &[(u16, &[u16])]) -> Vec<u8> {
    let mut bytes = imported_executable::pe32(code, "KERNEL32.dll", &["FindResourceA"]);
    bytes.resize(5120, 0);
    for (offset, value) in [
        (0x104, 60),
        (1044, 0x20a0),
        (1056, 0x2100),
        (1060, 0x20b0),
        (0x4a0, 0x2120),
        (0x4b0, 0x2120),
        (0x1b0, 4096),
        (0x108, 0x2300),
    ] {
        put(&mut bytes, offset, value);
    }
    bytes[0x500..0x50b].copy_from_slice(b"USER32.dll\0");
    bytes[0x522..0x52e].copy_from_slice(b"LoadStringA\0");
    let root = 0x700;
    for directory in [0, 24] {
        bytes[root + directory + 14..root + directory + 16].copy_from_slice(&1_u16.to_le_bytes());
    }
    bytes[root + 62..root + 64]
        .copy_from_slice(&u16::try_from(languages.len()).unwrap().to_le_bytes());
    for (offset, value) in [(16, 6), (20, 0x8000_0018), (40, 1), (44, 0x8000_0030)] {
        put(&mut bytes, root + offset, value);
    }
    let mut payload = 64 + languages.len() * 24;
    for (index, (language, units)) in languages.iter().enumerate() {
        let record = 64 + languages.len() * 8 + index * 16;
        put(&mut bytes, root + 64 + index * 8, u32::from(*language));
        put(
            &mut bytes,
            root + 68 + index * 8,
            u32::try_from(record).unwrap(),
        );
        put(
            &mut bytes,
            root + record,
            0x2300 + u32::try_from(payload).unwrap(),
        );
        put(
            &mut bytes,
            root + record + 4,
            u32::try_from(units.len() * 2).unwrap(),
        );
        for unit in *units {
            bytes[root + payload..root + payload + 2].copy_from_slice(&unit.to_le_bytes());
            payload += 2;
        }
    }
    put(&mut bytes, 0x10c, u32::try_from(payload).unwrap());
    bytes
}

pub fn guest() -> Vec<u8> {
    let content = block(&[&[65, 108, 112, 104, 97], &[66, 101, 116, 97]]);
    pe32(
        &[
            0x6a, 6, 0x6a, 1, 0x6a, 0, 0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x89, 0xc3, 0x6a, 4, 0x68,
            0x80, 0x21, 0x40, 0, 0x6a, 0, 0x6a, 0, 0xff, 0x15, 0xb0, 0x20, 0x40, 0, 0x8b, 0x0d,
            0x80, 0x21, 0x40, 0, 0xcc,
        ],
        &[(1033, &content)],
    )
}
