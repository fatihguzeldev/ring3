use super::imported_executable;

pub fn pe32() -> Vec<u8> {
    let mut bytes = imported_executable::pe32(
        &[
            0x68, 0x80, 0x22, 0x40, 0, 0x68, 0x80, 0x21, 0x40, 0, 0xff, 0x15, 0x60, 0x20, 0x40, 0,
            0x83, 0xc4, 8, 0xcc,
        ],
        "MSVCRT.dll",
        &["_stat"],
    );
    bytes[0x580..0x582].copy_from_slice(b".\0");
    bytes
}
