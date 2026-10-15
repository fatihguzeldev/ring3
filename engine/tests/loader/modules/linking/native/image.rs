pub enum Symbol<'a> {
    Name(&'a str),
    Ordinal(u16),
}

pub struct Import<'a> {
    pub module: &'a str,
    pub symbols: Vec<Symbol<'a>>,
}

fn put16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn file(rva: u32) -> usize {
    0x400 + (rva - 0x2000) as usize
}

fn string(bytes: &mut [u8], cursor: &mut u32, value: &str) -> u32 {
    let result = *cursor;
    bytes[file(*cursor)..file(*cursor) + value.len()].copy_from_slice(value.as_bytes());
    *cursor += value.len() as u32 + 1;
    result
}

pub fn image(
    dll: bool,
    base: u32,
    imports: &[Import<'_>],
    exports: &[(u16, Option<&str>, &str)],
) -> Vec<u8> {
    let mut bytes = vec![0; 0x1600];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 0x3c, 0x80);
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    for (offset, value) in [
        (0x84, 0x14c),
        (0x86, 3),
        (0x94, 224),
        (0x96, if dll { 0x2102 } else { 0x0102 }),
        (0x98, 0x10b),
        (0xdc, 3),
    ] {
        put16(&mut bytes, offset, value);
    }
    for (offset, value) in [
        (16, if dll { 0 } else { 0x1000 }),
        (28, base),
        (32, 4096),
        (36, 512),
        (56, 0x5000),
        (60, 512),
        (72, 0x100000),
        (76, 4096),
        (80, 0x100000),
        (84, 4096),
        (92, 16),
    ] {
        put32(&mut bytes, 0x98 + offset, value);
    }
    for (index, rva, raw, size, flags) in [
        (0, 0x1000, 0x200, 0x200, 0x60000020),
        (1, 0x2000, 0x400, 0x1000, 0xc0000040),
        (2, 0x4000, 0x1400, 0x200, 0x42000040),
    ] {
        let at = 0x178 + index * 40;
        bytes[at..at + 5].copy_from_slice(b"owned");
        for (offset, value) in [(8, size), (12, rva), (16, size), (20, raw), (36, flags)] {
            put32(&mut bytes, at + offset, value);
        }
    }
    bytes[0x200] = 0xc3;
    put32(&mut bytes, 0x210, base + 0x2f00);
    put32(&mut bytes, 0x98 + 96 + 5 * 8, 0x4000);
    put32(&mut bytes, 0x98 + 100 + 5 * 8, 12);
    put32(&mut bytes, 0x1400, 0x1000);
    put32(&mut bytes, 0x1404, 12);
    put16(&mut bytes, 0x1408, 0x3010);
    if !imports.is_empty() {
        put32(&mut bytes, 0x98 + 96 + 8, 0x2000);
        put32(&mut bytes, 0x98 + 100 + 8, (imports.len() as u32 + 1) * 20);
        let mut names = 0x2100;
        let mut symbols = 0x2600;
        let mut slot_index = 0;
        for (index, import) in imports.iter().enumerate() {
            let ilt = 0x2200 + slot_index * 4;
            let iat = 0x2400 + slot_index * 4;
            let module = string(&mut bytes, &mut names, import.module);
            for (offset, value) in [(0, ilt), (12, module), (16, iat)] {
                put32(&mut bytes, file(0x2000) + index * 20 + offset, value);
            }
            for symbol in &import.symbols {
                let value = match symbol {
                    Symbol::Ordinal(value) => 0x80000000 | u32::from(*value),
                    Symbol::Name(value) => {
                        symbols = (symbols + 1) & !1;
                        let name = symbols;
                        symbols += 2;
                        string(&mut bytes, &mut symbols, value);
                        name
                    }
                };
                put32(&mut bytes, file(0x2200 + slot_index * 4), value);
                put32(&mut bytes, file(0x2400 + slot_index * 4), value);
                slot_index += 1;
            }
            slot_index += 1;
        }
    }
    if !exports.is_empty() {
        let ordinal_base = exports.iter().map(|row| row.0).min().unwrap();
        let count = exports.iter().map(|row| row.0).max().unwrap() - ordinal_base + 1;
        let mut names: Vec<_> = exports.iter().filter(|row| row.1.is_some()).collect();
        names.sort_by_key(|row| row.1.unwrap());
        put32(&mut bytes, 0x98 + 96, 0x2800);
        put32(&mut bytes, 0x98 + 100, 0x400);
        for (offset, value) in [
            (16, u32::from(ordinal_base)),
            (20, u32::from(count)),
            (24, names.len() as u32),
            (28, 0x2840),
            (32, 0x2880),
            (36, 0x28c0),
        ] {
            put32(&mut bytes, file(0x2800) + offset, value);
        }
        let mut cursor = 0x2900;
        for &(ordinal, _, target) in exports {
            let target = if let Some(value) = target.strip_prefix('@') {
                u32::from_str_radix(value, 16).unwrap()
            } else {
                string(&mut bytes, &mut cursor, target)
            };
            put32(
                &mut bytes,
                file(0x2840) + usize::from(ordinal - ordinal_base) * 4,
                target,
            );
        }
        for (index, row) in names.into_iter().enumerate() {
            let rva = string(&mut bytes, &mut cursor, row.1.unwrap());
            put32(&mut bytes, file(0x2880) + index * 4, rva);
            put16(&mut bytes, file(0x28c0) + index * 2, row.0 - ordinal_base);
        }
    }
    bytes
}
