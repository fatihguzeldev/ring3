use super::imported_executable;

pub fn pe32() -> Vec<u8> {
    let mut code = Vec::new();
    for slot in [0_u32, 1, 1, 2, 2, 3, 0, 3] {
        code.extend_from_slice(&[0x68, 0x80, 0x21, 0x40, 0, 0xff, 0x15]);
        code.extend_from_slice(&(0x0040_2060 + slot * 4).to_le_bytes());
    }
    code.push(0xcc);
    imported_executable::pe32(
        &code,
        "kernel32.dll",
        &[
            "InitializeCriticalSection",
            "TryEnterCriticalSection",
            "LeaveCriticalSection",
            "DeleteCriticalSection",
        ],
    )
}
