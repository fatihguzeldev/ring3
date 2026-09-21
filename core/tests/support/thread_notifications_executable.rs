use super::dll_executable;

pub fn dll() -> Vec<u8> {
    let base = 0x5000_0000_u32;
    let mut code = vec![0x8b, 0x44, 0x24, 4, 0x50, 0xff, 0x15];
    code.extend_from_slice(&(base + 0x2060).to_le_bytes());
    code.extend_from_slice(&[0xc2, 12, 0]);
    let mut bytes = dll_executable::dll(base, &code, Some("KERNEL32.dll"));
    let name = b"DisableThreadLibraryCalls\0";
    bytes[1218..1218 + name.len()].copy_from_slice(name);
    bytes
}
