#[path = "../../../../core/tests/support/executable.rs"]
mod executable;
#[path = "../../../../core/tests/support/imported_executable.rs"]
mod imported_executable;

pub fn executable() -> Vec<u8> {
    let mut code = Vec::new();
    for value in [0_u32, 0, 3, 0, 1, 0x8000_0000, 0x0040_2180] {
        code.push(0x68);
        code.extend(value.to_le_bytes());
    }
    code.extend([0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x89, 0xc6]);
    for value in [0_u32, 0x0040_21b0, 4, 0x0040_21a0] {
        code.push(0x68);
        code.extend(value.to_le_bytes());
    }
    code.extend([0x56, 0xff, 0x15, 0x64, 0x20, 0x40, 0]);
    code.extend([0x81, 0x3d, 0xa0, 0x21, 0x40, 0, b'd', b'a', b't', b'a']);
    code.extend([0x75, 4, 0x6a, 42, 0xeb, 2, 0x6a, 13]);
    code.extend([0xff, 0x15, 0x68, 0x20, 0x40, 0, 0xcc]);
    let mut bytes = imported_executable::pe32(
        &code,
        "kernel32.dll",
        &["CreateFileA", "ReadFile", "ExitProcess"],
    );
    bytes[0x580..0x589].copy_from_slice(b"C:\\a.bin\0");
    bytes
}
