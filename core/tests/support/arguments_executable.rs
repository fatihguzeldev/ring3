use super::imported_executable;

pub fn pe32(wildcard: u32, new_mode: u32) -> Vec<u8> {
    let mut code = vec![0xc7, 0x05, 0x00, 0x23, 0x40, 0];
    code.extend_from_slice(&new_mode.to_le_bytes());
    code.extend_from_slice(&[0x68, 0, 0x23, 0x40, 0, 0x68]);
    code.extend_from_slice(&wildcard.to_le_bytes());
    code.extend_from_slice(&[
        0x68, 0x0c, 0x23, 0x40, 0, 0x68, 8, 0x23, 0x40, 0, 0x68, 4, 0x23, 0x40, 0, 0xff, 0x15,
        0x60, 0x20, 0x40, 0, 0x83, 0xc4, 20, 0xcc,
    ]);
    imported_executable::pe32(&code, "MSVCRT.dll", &["__getmainargs"])
}
