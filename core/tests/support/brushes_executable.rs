use super::executable;

pub fn pe32(code: &[u8]) -> Vec<u8> {
    let mut bytes = executable::pe32(code);
    bytes[1024..].fill(0);
    for (offset, value) in [
        (0x100, 0x2000_u32),
        (0x104, 60),
        (1024, 0x2040),
        (1036, 0x2080),
        (1040, 0x2060),
        (1044, 0x2050),
        (1056, 0x2090),
        (1060, 0x2070),
        (1088, 0x20a0),
        (1092, 0x20c0),
        (1104, 0x20e0),
        (1108, 0x2100),
        (1120, 0x20a0),
        (1124, 0x20c0),
        (1136, 0x20e0),
        (1140, 0x2100),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    for (offset, name) in [
        (1152, "uSeR32.dll"),
        (1168, "Gdi32.dll"),
        (1186, "GetSysColorBrush"),
        (1218, "GetSysColor"),
        (1250, "GetObjectA"),
        (1282, "DeleteObject"),
    ] {
        bytes[offset..offset + name.len()].copy_from_slice(name.as_bytes());
    }
    bytes
}

pub fn lifecycle() -> Vec<u8> {
    pe32(&[
        0x6a, 15, 0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x89, 0xc3, 0x68, 0x80, 0x21, 0x40, 0, 0x6a, 12,
        0x53, 0xff, 0x15, 0x70, 0x20, 0x40, 0, 0x89, 0xc6, 0x53, 0xff, 0x15, 0x74, 0x20, 0x40, 0,
        0x89, 0xc7, 0x6a, 15, 0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x8b, 0x15, 0x84, 0x21, 0x40, 0,
        0xcc,
    ])
}
