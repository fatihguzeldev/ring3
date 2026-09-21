use super::imported_executable;

pub fn pe32() -> Vec<u8> {
    let mut bytes = imported_executable::pe32(
        &[
            0x6a, 16, 0x68, 0x80, 0x22, 0x40, 0, 0x68, 0x80, 0x21, 0x40, 0, 0xff, 0x15, 0x60, 0x20,
            0x40, 0, 0xcc,
        ],
        "KERNEL32.dll",
        &["GetEnvironmentVariableA"],
    );
    bytes[0x580..0x585].copy_from_slice(b"DEMO\0");
    bytes
}
