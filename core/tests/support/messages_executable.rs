use super::imported_executable;

pub fn pe32() -> Vec<u8> {
    let mut bytes = imported_executable::pe32(
        &[
            0x68, 0x80, 0x21, 0x40, 0, 0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x89, 0xc3, 0x68, 0xa0,
            0x21, 0x40, 0, 0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x89, 0xc1, 0x68, 0xc0, 0x21, 0x40, 0,
            0xff, 0x15, 0x60, 0x20, 0x40, 0, 0xcc,
        ],
        "USER32.dll",
        &["RegisterWindowMessageA"],
    );
    for (offset, name) in [
        (1408, b"Ring3.Alpha\0".as_slice()),
        (1440, b"RING3.ALPHA\0"),
        (1472, b"Ring3.Beta\0"),
    ] {
        bytes[offset..offset + name.len()].copy_from_slice(name);
    }
    bytes
}
