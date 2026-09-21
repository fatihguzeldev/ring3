use super::imported_executable;

pub fn pe32() -> Vec<u8> {
    imported_executable::pe32(
        &[
            0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x89, 0xc3, 0xff, 0x15, 0x60, 0x20, 0x40, 0, 0xcc,
        ],
        "WiNmM.dll",
        &["timeGetTime"],
    )
}
