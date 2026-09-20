use super::imported_executable;

fn put(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

pub fn pe32(legacy: bool, unknown_api: bool) -> Vec<u8> {
    let mut code = vec![
        0xff, 0x15, 0x80, 0x21, 0x40, 0, 0x89, 0xc3, 0xff, 0x15, 0x80, 0x21, 0x40, 0, 0xcc,
    ];
    code.resize(128, 0xcc);
    code.extend_from_slice(&[0xb8, 42, 0, 0, 0, 0xc3]);
    code.resize(192, 0xcc);
    if unknown_api {
        code.extend_from_slice(&[0x6a, 0, 0xff, 0x15, 0x60, 0x20, 0x40, 0]);
    }
    code.extend_from_slice(&[
        0x83, 0x05, 0x90, 0x21, 0x40, 0, 1, 0xc7, 0x05, 0x80, 0x21, 0x40, 0, 0x80, 0x10, 0x40, 0,
        0xb8, 0x80, 0x10, 0x40, 0, 0xff, 0xe0,
    ]);
    let names: &[&str] = if unknown_api {
        &["MissingDelayApi"]
    } else {
        &[]
    };
    let mut bytes = imported_executable::pe32(&code, "KERNEL32.dll", names);
    bytes.resize(2048, 0);
    if !unknown_api {
        put(&mut bytes, 0x100, 0);
        put(&mut bytes, 0x104, 0);
    }
    put(&mut bytes, 0x1b0, 1024);
    put(&mut bytes, 0x160, 0x2200);
    put(&mut bytes, 0x164, 64);
    put(&mut bytes, 1408, 0x0040_10c0);
    let bias = if legacy { 0x0040_0000 } else { 0 };
    for (offset, value) in [
        (1536, u32::from(!legacy)),
        (1540, bias + 0x2240),
        (1544, bias + 0x2188),
        (1548, bias + 0x2180),
        (1552, bias + 0x2250),
        (1616, bias + 0x2260),
    ] {
        put(&mut bytes, offset, value);
    }
    bytes[1600..1612].copy_from_slice(b"missing.dll\0");
    bytes[1634..1640].copy_from_slice(b"probe\0");
    bytes
}
