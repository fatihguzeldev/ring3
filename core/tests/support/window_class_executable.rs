use super::imported_executable;

pub fn pe32() -> Vec<u8> {
    let mut bytes = imported_executable::pe32(
        &[
            0x68, 0xc0, 0x21, 0x40, 0, 0xff, 0x15, 0x64, 0x20, 0x40, 0, 0x89, 0xc3, 0x68, 0x80,
            0x22, 0x40, 0, 0x68, 0x80, 0x21, 0x40, 0, 0x68, 0, 0, 0x40, 0, 0xff, 0x15, 0x60, 0x20,
            0x40, 0, 0x68, 0, 0, 0x40, 0, 0x53, 0xff, 0x15, 0x68, 0x20, 0x40, 0, 0xcc,
        ],
        "UsEr32.dll",
        &["GetClassInfoA", "RegisterClassA", "UnregisterClassA"],
    );
    bytes[0x580..0x585].copy_from_slice(b"demo\0");
    for (i, word) in [
        3_u32,
        0x0040_1040,
        0,
        0,
        0x0040_0000,
        0,
        0,
        0,
        0,
        0x0040_2180,
    ]
    .iter()
    .enumerate()
    {
        bytes[0x5c0 + i * 4..0x5c4 + i * 4].copy_from_slice(&word.to_le_bytes());
    }
    bytes
}
