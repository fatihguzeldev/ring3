use super::imported_executable;

pub fn pe32(procedure: &[u8]) -> Vec<u8> {
    let mut code = vec![0x68, 0xc0, 0x21, 0x40, 0, 0xff, 0x15, 0x60, 0x20, 0x40, 0];
    for value in [
        0x1234_u32,
        0x0040_0000,
        0,
        0,
        90,
        130,
        20,
        10,
        0x00ca_0000,
        0x0040_2190,
        0x0040_2180,
        0,
    ] {
        code.push(0x68);
        code.extend_from_slice(&value.to_le_bytes());
    }
    code.extend_from_slice(&[
        0xff, 0x15, 0x64, 0x20, 0x40, 0, 0x89, 0xc3, 0x50, 0xff, 0x15, 0x6c, 0x20, 0x40, 0, 0xcc,
    ]);
    code.resize(0x100, 0xcc);
    code.extend_from_slice(procedure);
    let mut bytes = imported_executable::pe32(
        &code,
        "USER32.dll",
        &[
            "RegisterClassA",
            "CreateWindowExA",
            "DefWindowProcA",
            "IsWindow",
        ],
    );
    bytes[0x580..0x587].copy_from_slice(b"sample\0");
    bytes[0x590..0x596].copy_from_slice(b"title\0");
    for (index, value) in [
        3_u32,
        0x0040_1100,
        0,
        0,
        0x0040_0000,
        0,
        0,
        0,
        0,
        0x0040_2180,
    ]
    .iter()
    .enumerate()
    {
        bytes[0x5c0 + index * 4..0x5c4 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

pub fn guest() -> Vec<u8> {
    pe32(&default_procedure())
}

pub fn default_procedure() -> Vec<u8> {
    let mut procedure = Vec::new();
    for _ in 0..4 {
        procedure.extend_from_slice(&[0xff, 0x74, 0x24, 16]);
    }
    procedure.extend_from_slice(&[0xff, 0x15, 0x68, 0x20, 0x40, 0, 0xc2, 16, 0]);
    procedure
}
