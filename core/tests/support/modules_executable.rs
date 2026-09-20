use super::imported_executable;

pub fn pe32() -> Vec<u8> {
    let mut code = Vec::new();
    for (pointer, slot, save) in [(0x0040_2180_u32, 0, 0xc3), (0x0040_2190, 1, 0xc6)] {
        code.push(0x68);
        code.extend_from_slice(&pointer.to_le_bytes());
        code.extend_from_slice(&[0xff, 0x15]);
        code.extend_from_slice(&(0x0040_2060_u32 + slot * 4).to_le_bytes());
        code.extend_from_slice(&[0x89, save]);
    }
    code.extend_from_slice(&[0x53, 0xff, 0x15, 0x68, 0x20, 0x40, 0, 0x89, 0xc7]);
    code.extend_from_slice(&[
        0x68, 0xa0, 0x21, 0x40, 0, 0xff, 0x15, 0x64, 0x20, 0x40, 0, 0xcc,
    ]);
    let mut bytes = imported_executable::pe32(
        &code,
        "kernel32.dll",
        &["LoadLibraryA", "GetModuleHandleA", "FreeLibrary"],
    );
    bytes[1408..1419].copy_from_slice(b"MSVCRT.DLL\0");
    bytes[1424..1431].copy_from_slice(b"msvcrt\0");
    bytes[1440..1448].copy_from_slice(b"missing\0");
    bytes
}
