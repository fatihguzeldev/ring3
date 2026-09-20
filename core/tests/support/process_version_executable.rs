use super::imported_executable;

pub fn pe32(major: u16, minor: u16) -> Vec<u8> {
    let mut bytes = imported_executable::pe32(
        &[0x6a, 0, 0xff, 0x15, 0x60, 0x20, 0x40, 0, 0xcc],
        "KERNEL32.dll",
        &["GetProcessVersion"],
    );
    for (offset, value) in [
        (0xc0, 17_u16),
        (0xc2, 18),
        (0xc4, 19),
        (0xc6, 20),
        (0xc8, major),
        (0xca, minor),
    ] {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}
