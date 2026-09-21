use super::imported_executable;

pub fn pe32() -> Vec<u8> {
    image([b"MSVCRT.DLL\0", b"msvcrt\0", b"missing\0"])
}

pub fn absolute() -> Vec<u8> {
    image([
        b"C:\\Windows\\System32\\MSVCRT\0",
        b"c:\\windows\\system32\\msvcrt.dll.\0",
        b"C:\\Other\\msvcrt.dll\0",
    ])
}

fn image(names: [&[u8]; 3]) -> Vec<u8> {
    let mut code = Vec::new();
    for (pointer, slot, save) in [(0x0040_2180_u32, 0, 0xc3), (0x0040_21a0, 1, 0xc6)] {
        code.push(0x68);
        code.extend_from_slice(&pointer.to_le_bytes());
        code.extend_from_slice(&[0xff, 0x15]);
        code.extend_from_slice(&(0x0040_2060_u32 + slot * 4).to_le_bytes());
        code.extend_from_slice(&[0x89, save]);
    }
    code.extend_from_slice(&[0x53, 0xff, 0x15, 0x68, 0x20, 0x40, 0, 0x89, 0xc7]);
    code.extend_from_slice(&[
        0x68, 0xc0, 0x21, 0x40, 0, 0xff, 0x15, 0x64, 0x20, 0x40, 0, 0xcc,
    ]);
    let mut bytes = imported_executable::pe32(
        &code,
        "kernel32.dll",
        &["LoadLibraryA", "GetModuleHandleA", "FreeLibrary"],
    );
    for (index, name) in names.into_iter().enumerate() {
        let start = 1408 + index * 32;
        assert!(name.len() <= 32);
        bytes[start..start + name.len()].copy_from_slice(name);
    }
    bytes
}
