use super::imported_executable;

pub fn pe32() -> Vec<u8> {
    let mut code = Vec::new();
    for word in [4_u32, 0x0040_21b0, 4, 0, 0x0040_21a0, 0x8000_0001] {
        code.push(0x68);
        code.extend(word.to_le_bytes());
    }
    code.extend([0xff, 0x15, 0x60, 0x20, 0x40, 0]);
    for word in [
        0x0040_2184_u32,
        0x0040_2188,
        0x0040_2180,
        0,
        0x0040_21a0,
        0x8000_0001,
    ] {
        code.push(0x68);
        code.extend(word.to_le_bytes());
    }
    code.extend([0xff, 0x15, 0x64, 0x20, 0x40, 0]);
    code.extend([0xa1, 0x88, 0x21, 0x40, 0, 0xcc]);
    let mut bytes = imported_executable::pe32(
        &code,
        "ADVAPI32.dll",
        &["RegSetValueExA", "RegQueryValueExA"],
    );
    bytes[0x584..0x588].copy_from_slice(&4_u32.to_le_bytes());
    bytes[0x5a0..0x5a8].copy_from_slice(b"Setting\0");
    bytes[0x5b0..0x5b4].copy_from_slice(&0x7856_3412_u32.to_le_bytes());
    bytes
}
