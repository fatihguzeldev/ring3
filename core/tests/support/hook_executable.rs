use super::imported_executable;

pub fn pe32() -> Vec<u8> {
    registration(u32::MAX, 0, 1)
}

pub fn keyboard() -> Vec<u8> {
    registration(13, 0x0040_0000, 0)
}

pub fn thread_keyboard() -> Vec<u8> {
    registration(2, 0x0040_0000, 1)
}

pub fn cbt() -> Vec<u8> {
    registration(5, 0, 1)
}

fn registration(kind: u32, module: u32, thread: u32) -> Vec<u8> {
    let mut code = Vec::new();
    for argument in [thread, module, 0x0040_1040, kind] {
        code.push(0x68);
        code.extend_from_slice(&argument.to_le_bytes());
    }
    code.extend_from_slice(&[
        0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x89, 0xc3, 0x53, 0xff, 0x15, 0x64, 0x20, 0x40, 0, 0x53,
        0xff, 0x15, 0x64, 0x20, 0x40, 0, 0xcc,
    ]);
    imported_executable::pe32(
        &code,
        "UsEr32.dll",
        &["SetWindowsHookExA", "UnhookWindowsHookEx"],
    )
}
