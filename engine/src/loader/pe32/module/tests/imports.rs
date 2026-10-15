use super::{support as pe, *};

fn image() -> Vec<u8> {
    let mut bytes = pe::empty_data();
    pe::directory(&mut bytes, 1, 0x2000, 60);
    for (at, lookup, name, iat) in [
        (0x2000, 0, 0x2050, 0x20a0),
        (0x2014, 0x20b0, 0x2060, 0x20c0),
    ] {
        pe::word(&mut bytes, at, lookup);
        pe::word(&mut bytes, at + 12, name);
        pe::word(&mut bytes, at + 16, iat);
    }
    pe::data(&mut bytes, 0x2050, b"first.dll\0");
    pe::data(&mut bytes, 0x2060, b"second.dll\0");
    pe::data(&mut bytes, 0x2080, b"\0\0named\0");
    pe::word(&mut bytes, 0x20a0, 0x8000_0023);
    pe::word(&mut bytes, 0x20a4, 0x2080);
    pe::word(&mut bytes, 0x20b0, 0x8000_0007);
    bytes
}

#[test]
fn multiple_descriptors_preserve_ordinal_name_and_oft_zero_iat_slots() {
    let parsed = parse_module(&image()).unwrap();
    assert_eq!(
        parsed.imports,
        vec![
            ImportModule32 {
                name: "first.dll".into(),
                slots: vec![
                    ImportSlot32 {
                        iat_rva: 0x20a0,
                        symbol: ImportSymbol32::Ordinal(0x23)
                    },
                    ImportSlot32 {
                        iat_rva: 0x20a4,
                        symbol: ImportSymbol32::Name("named".into())
                    },
                ]
            },
            ImportModule32 {
                name: "second.dll".into(),
                slots: vec![ImportSlot32 {
                    iat_rva: 0x20c0,
                    symbol: ImportSymbol32::Ordinal(7)
                }]
            },
        ]
    );
}

#[test]
fn unterminated_descriptors_reserved_ordinal_bits_and_overlapping_iat_are_rejected() {
    let mut bytes = image();
    pe::directory(&mut bytes, 1, 0x2000, 40);
    assert_eq!(parse_module(&bytes).err(), Some(LoadError::Malformed));
    let mut bytes = image();
    pe::word(&mut bytes, 0x20a0, 0x8001_0023);
    assert_eq!(parse_module(&bytes).err(), Some(LoadError::Malformed));
    let mut bytes = image();
    pe::word(&mut bytes, 0x2014 + 16, 0x20a0);
    assert_eq!(parse_module(&bytes).err(), Some(LoadError::Malformed));
}
