use super::imported_executable;

pub fn pe32(procedure: &[u8]) -> Vec<u8> {
    let mut code = Vec::new();
    for value in [19_u32, 17, 13, 11, 0x0040_1060] {
        code.push(0x68);
        code.extend_from_slice(&value.to_le_bytes());
    }
    code.extend_from_slice(&[0xff, 0x15, 0x60, 0x20, 0x40, 0, 0xcc]);
    code.resize(0x60, 0xcc);
    code.extend_from_slice(procedure);
    imported_executable::pe32(&code, "USER32.dll", &["CallWindowProcA"])
}

pub fn guest() -> Vec<u8> {
    pe32(&[
        0x8b, 0x44, 0x24, 4, 0x03, 0x44, 0x24, 8, 0x03, 0x44, 0x24, 12, 0x03, 0x44, 0x24, 16, 0xc2,
        16, 0,
    ])
}

pub fn nested() -> Vec<u8> {
    let mut procedure = Vec::new();
    for _ in 0..4 {
        procedure.extend_from_slice(&[0xff, 0x74, 0x24, 16]);
    }
    procedure.extend_from_slice(&[
        0x68, 0xc0, 0x10, 0x40, 0, 0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x83, 0xc0, 1, 0xc2, 16, 0,
    ]);
    procedure.resize(0x60, 0xcc);
    procedure.extend_from_slice(&[
        0x6a, 77, 0xb8, 0, 0, 0, 0x70, 0xff, 0xd0, 0x8b, 0x44, 0x24, 4, 0x03, 0x44, 0x24, 8, 0x03,
        0x44, 0x24, 12, 0x03, 0x44, 0x24, 16, 0xc2, 16, 0,
    ]);
    pe32(&procedure)
}
