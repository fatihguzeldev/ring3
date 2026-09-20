use super::imported_executable;

pub fn pe32(application_type: u32) -> Vec<u8> {
    let mut code = vec![0xb8, 0x34, 0x12, 0, 0, 0x68];
    code.extend_from_slice(&application_type.to_le_bytes());
    code.extend_from_slice(&[
        0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x59, 0xff, 0x15, 0x64, 0x20, 0x40, 0, 0xc7, 0, 32, 0, 0,
        0, 0x83, 0x08, 3, 0xff, 0x15, 0x68, 0x20, 0x40, 0, 0xc7, 0, 7, 0, 0, 0, 0x8b, 0x10, 0xa1,
        0x6c, 0x20, 0x40, 0, 0x8b, 0x18, 0xcc,
    ]);
    imported_executable::pe32(
        &code,
        "mSvCrT.dLl",
        &[
            "__set_app_type",
            "__p__fmode",
            "__p__commode",
            "_adjust_fdiv",
        ],
    )
}
