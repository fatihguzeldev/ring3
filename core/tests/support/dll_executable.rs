use super::imported_executable;

pub fn put(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

pub fn dll(base: u32, attach: &[u8], dependency: Option<&str>) -> Vec<u8> {
    let mut code = attach.to_vec();
    code.resize(128, 0xcc);
    code.push(0xa1);
    code.extend_from_slice(&(base + 0x2180).to_le_bytes());
    code.extend_from_slice(&[0x83, 0xc0, 1, 0xc3]);
    let mut bytes = imported_executable::pe32(&code, dependency.unwrap_or("unused"), &["state"]);
    if dependency.is_none() {
        put(&mut bytes, 0x100, 0);
        put(&mut bytes, 0x104, 0);
    }
    bytes.resize(2048, 0);
    bytes[0x96..0x98].copy_from_slice(&0x2102_u16.to_le_bytes());
    put(&mut bytes, 0xb4, base);
    put(&mut bytes, 0x1b0, 1024);
    put(&mut bytes, 0xf8, 0x2200);
    put(&mut bytes, 0xfc, 128);
    for (offset, value) in [
        (1552, 7),
        (1556, 2),
        (1560, 2),
        (1564, 0x2240),
        (1568, 0x2248),
        (1572, 0x2250),
        (1600, 0x1080),
        (1604, 0x2180),
        (1608, 0x2260),
        (1612, 0x2270),
    ] {
        put(&mut bytes, offset, value);
    }
    bytes[1618..1620].copy_from_slice(&1_u16.to_le_bytes());
    bytes[1632..1638].copy_from_slice(b"value\0");
    bytes[1648..1654].copy_from_slice(b"state\0");
    bytes
}

pub fn attach(base: u32, value: u32, success: bool) -> Vec<u8> {
    let mut code = Vec::new();
    for (stack, target) in [(4, 0x2190), (8, 0x2194), (12, 0x2198)] {
        code.extend_from_slice(&[0x8b, 0x44, 0x24, stack, 0xa3]);
        code.extend_from_slice(&(base + target).to_le_bytes());
    }
    code.extend_from_slice(&[0xc7, 0x05]);
    code.extend_from_slice(&(base + 0x2180).to_le_bytes());
    code.extend_from_slice(&value.to_le_bytes());
    code.push(0xb8);
    code.extend_from_slice(&u32::from(success).to_le_bytes());
    code.extend_from_slice(&[0xc2, 12, 0]);
    code
}

pub fn exe(module: &str) -> Vec<u8> {
    let mut bytes = imported_executable::pe32(
        &[
            0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x89, 0xc3, 0xff, 0x15, 0x68, 0x20, 0x40, 0, 0xcc,
        ],
        module,
        &["value", "state", "ordinal"],
    );
    put(&mut bytes, 1096, 0x8000_0007);
    bytes
}
