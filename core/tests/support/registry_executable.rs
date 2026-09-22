use super::imported_executable;

pub fn pe32() -> Vec<u8> {
    let mut code = Vec::new();
    for word in [0x0040_2180_u32, 0x2001f, 0, 0x0040_21a0, 0x8000_0001] {
        code.push(0x68);
        code.extend(word.to_le_bytes());
    }
    code.extend([0xff, 0x15, 0x60, 0x20, 0x40, 0]);
    code.extend([0x8b, 0x1d, 0x80, 0x21, 0x40, 0]);
    for word in [
        0x0040_2184_u32,
        0x0040_2180,
        0,
        0x2001f,
        0,
        0,
        0,
        0x0040_21b0,
    ] {
        code.push(0x68);
        code.extend(word.to_le_bytes());
    }
    code.push(0x53);
    code.extend([0xff, 0x15, 0x64, 0x20, 0x40, 0]);
    code.extend([0x8b, 0x35, 0x80, 0x21, 0x40, 0]);
    code.push(0x56);
    code.extend([0xff, 0x15, 0x68, 0x20, 0x40, 0]);
    code.extend([0xa1, 0x84, 0x21, 0x40, 0, 0xcc]);
    let mut bytes = imported_executable::pe32(
        &code,
        "advapi32.dll",
        &["RegOpenKeyExA", "RegCreateKeyExA", "RegCloseKey"],
    );
    bytes[0x5a0..0x5a9].copy_from_slice(b"Software\0");
    bytes[0x5b0..0x5c1].copy_from_slice(b"Example\\Settings\0");
    bytes
}
