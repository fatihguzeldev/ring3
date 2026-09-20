pub fn pe32(code: &[u8]) -> Vec<u8> {
    assert!(code.len() <= 512);
    let mut bytes = vec![0; 1536];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x3c..0x40].copy_from_slice(&0x80_u32.to_le_bytes());
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    for (offset, value) in [
        (0x84, 0x14c_u16),
        (0x86, 2),
        (0x94, 224),
        (0x96, 0x102),
        (0x98, 0x10b),
        (0xdc, 3),
    ] {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    for (offset, value) in [
        (0xa8, 0x1000_u32),
        (0xb4, 0x40_0000),
        (0xb8, 4096),
        (0xbc, 512),
        (0xd0, 0x3000),
        (0xd4, 512),
        (0xf4, 16),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    for (offset, name, rva, raw, flags) in [
        (0x178, b".text\0\0\0", 0x1000_u32, 512_u32, 0x6000_0020_u32),
        (0x1a0, b".data\0\0\0", 0x2000, 1024, 0xc000_0040),
    ] {
        bytes[offset..offset + 8].copy_from_slice(name);
        for (field, value) in [(8, 4096), (12, rva), (16, 512), (20, raw), (36, flags)] {
            bytes[offset + field..offset + field + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    bytes[512..512 + code.len()].copy_from_slice(code);
    bytes[1024] = 17;
    bytes
}
