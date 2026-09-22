use super::imported_executable;

pub fn pe32() -> Vec<u8> {
    let mut code = Vec::new();
    for (import, args) in [
        (
            0x0040_2060_u32,
            [0_u32, 0x0040_21a0, 1, 0x0040_2180, 0x8000_0001],
        ),
        (0x0040_2064, [0x0040_21c0, 1, 0, 0x0040_2180, 0x8000_0001]),
    ] {
        for arg in args {
            code.push(0x68);
            code.extend_from_slice(&arg.to_le_bytes());
        }
        code.extend_from_slice(&[0xff, 0x15]);
        code.extend_from_slice(&import.to_le_bytes());
    }
    for arg in [0x0040_21c8_u32, 0x0040_2220, 0x0040_21c4, 0, 0] {
        code.push(0x68);
        code.extend_from_slice(&arg.to_le_bytes());
    }
    code.extend_from_slice(&[
        0xff, 0x35, 0xc0, 0x21, 0x40, 0, 0xff, 0x15, 0x68, 0x20, 0x40, 0, 0xcc,
    ]);
    let mut bytes = imported_executable::pe32(
        &code,
        "ADVAPI32.dll",
        &["RegSetValueA", "RegOpenKeyExA", "RegQueryValueExA"],
    );
    bytes[1408..1425].copy_from_slice(b"software\\example\0");
    bytes[1440..1446].copy_from_slice(b"hello\0");
    bytes[1480..1484].copy_from_slice(&6_u32.to_le_bytes());
    bytes
}
