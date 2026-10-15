pub(super) const BASE: u32 = 0x0040_0000;
pub(super) const COFF: usize = 0x84;
pub(super) const OPTIONAL: usize = 0x98;

pub(super) fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

pub(super) fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

pub(super) fn image(dll: bool, small: bool) -> Vec<u8> {
    let mut bytes = vec![0; 0x800];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 0x3c, 0x80);
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    put16(&mut bytes, COFF, 0x14c);
    put16(&mut bytes, COFF + 2, 2);
    put16(&mut bytes, COFF + 16, 224);
    put16(&mut bytes, COFF + 18, if dll { 0x2102 } else { 0x0102 });
    put16(&mut bytes, OPTIONAL, 0x10b);
    put32(
        &mut bytes,
        OPTIONAL + 16,
        if dll {
            0
        } else if small {
            0x200
        } else {
            0x1000
        },
    );
    put32(&mut bytes, OPTIONAL + 28, BASE);
    put32(
        &mut bytes,
        OPTIONAL + 32,
        if small { 0x200 } else { 0x1000 },
    );
    put32(&mut bytes, OPTIONAL + 36, 0x200);
    put32(
        &mut bytes,
        OPTIONAL + 56,
        if small { 0x800 } else { 0x3000 },
    );
    put32(&mut bytes, OPTIONAL + 60, 0x200);
    put16(&mut bytes, OPTIONAL + 68, 3);
    put16(&mut bytes, OPTIONAL + 70, 0x140);
    put32(&mut bytes, OPTIONAL + 72, 0x100000);
    put32(&mut bytes, OPTIONAL + 76, 0x1000);
    put32(&mut bytes, OPTIONAL + 80, 0x100000);
    put32(&mut bytes, OPTIONAL + 84, 0x1000);
    put32(&mut bytes, OPTIONAL + 92, 16);
    for (index, rva, raw, flags) in [
        (0, if small { 0x200 } else { 0x1000 }, 0x200, 0x6000_0020),
        (1, if small { 0x400 } else { 0x2000 }, 0x400, 0xc000_0040),
    ] {
        let at = OPTIONAL + 224 + index * 40;
        put32(&mut bytes, at + 8, if small { 0x200 } else { 0x400 });
        put32(&mut bytes, at + 12, rva);
        put32(&mut bytes, at + 16, 0x200);
        put32(&mut bytes, at + 20, raw);
        put32(&mut bytes, at + 36, flags);
    }
    bytes[0x200] = 0xc3;
    bytes[0x400..0x600].fill(0x5a);
    bytes
}

pub(super) fn directory(bytes: &mut [u8], index: usize, rva: u32, size: u32) {
    put32(bytes, OPTIONAL + 96 + index * 8, rva);
    put32(bytes, OPTIONAL + 100 + index * 8, size);
}

pub(super) fn offset(rva: u32) -> usize {
    match rva {
        0..0x200 => rva as usize,
        0x1000..0x1200 => 0x200 + (rva - 0x1000) as usize,
        0x2000..0x2200 => 0x400 + (rva - 0x2000) as usize,
        _ => panic!("test file rva outside initialized bytes"),
    }
}

pub(super) fn word(bytes: &mut [u8], rva: u32, value: u32) {
    put32(bytes, offset(rva), value);
}

pub(super) fn data(bytes: &mut [u8], rva: u32, value: &[u8]) {
    let at = offset(rva);
    bytes[at..at + value.len()].copy_from_slice(value);
}

pub(super) fn empty_data() -> Vec<u8> {
    let mut bytes = image(true, false);
    bytes[0x400..0x600].fill(0);
    bytes
}
