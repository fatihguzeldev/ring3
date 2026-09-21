use super::imported_executable;

pub fn pe32() -> Vec<u8> {
    let mut bytes = imported_executable::pe32(
        &[
            0x68, 0xc0, 0x21, 0x40, 0, 0x68, 0x80, 0x21, 0x40, 0, 0x6a, 32, 0x68, 0, 0x22, 0x40, 0,
            0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x83, 0xc4, 16, 0xcc,
        ],
        "mSvCrT.dll",
        &["_vsnprintf"],
    );
    bytes[0x580..0x58c].copy_from_slice(b"%s:%d:%X:%%\0");
    bytes[0x5a0..0x5a3].copy_from_slice(b"ok\0");
    for (index, value) in [0x0040_21a0_u32, (-42_i32).cast_unsigned(), 0xabcd]
        .into_iter()
        .enumerate()
    {
        bytes[0x5c0 + index * 4..0x5c4 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}
