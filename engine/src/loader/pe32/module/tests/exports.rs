use super::{support as pe, *};

fn image() -> Vec<u8> {
    let mut bytes = pe::empty_data();
    pe::directory(&mut bytes, 0, 0x2000, 0x100);
    for (offset, value) in [
        (16, 10),
        (20, 4),
        (24, 3),
        (28, 0x2050),
        (32, 0x2080),
        (36, 0x2090),
    ] {
        pe::word(&mut bytes, 0x2000 + offset, value);
    }
    for (index, value) in [0x1000, 0, 0x2200, 0x20c0].into_iter().enumerate() {
        pe::word(&mut bytes, 0x2050 + index as u32 * 4, value);
    }
    for (index, (at, name, ordinal)) in
        [(0x20a0, "Code", 0), (0x20a8, "Data", 2), (0x20b0, "Hop", 3)]
            .into_iter()
            .enumerate()
    {
        pe::word(&mut bytes, 0x2080 + index as u32 * 4, at);
        pe::data(&mut bytes, at, format!("{name}\0").as_bytes());
        pe::put16(&mut bytes, pe::offset(0x2090) + index * 2, ordinal);
    }
    pe::data(&mut bytes, 0x20c0, b"other.#7\0");
    bytes
}

#[test]
fn exports_distinguish_code_data_holes_ordinals_and_forwarders() {
    let image = parse_module(&image()).unwrap();
    assert_eq!(
        image.exports.lookup(&ImportSymbol32::Name("Code".into())),
        Some(&ExportTarget32::Address(0x1000))
    );
    assert_eq!(
        image.exports.lookup(&ImportSymbol32::Ordinal(12)),
        Some(&ExportTarget32::Address(0x2200))
    );
    assert_eq!(
        image.exports.lookup(&ImportSymbol32::Name("Hop".into())),
        Some(&ExportTarget32::Forwarder("other.#7".into()))
    );
    assert_eq!(image.exports.lookup(&ImportSymbol32::Ordinal(11)), None);
    assert_eq!(
        image.exports.lookup(&ImportSymbol32::Name("code".into())),
        None
    );
}

#[test]
fn bad_name_ordinal_and_forwarder_escaping_directory_are_rejected() {
    let mut bytes = image();
    pe::put16(&mut bytes, pe::offset(0x2090), 4);
    assert_eq!(parse_module(&bytes).err(), Some(LoadError::Malformed));
    let mut bytes = image();
    bytes[pe::offset(0x20c0)..pe::offset(0x2100)].fill(b'x');
    assert_eq!(parse_module(&bytes).err(), Some(LoadError::Malformed));
}

#[test]
fn unsorted_export_names_do_not_create_a_different_initial_and_live_lookup() {
    let mut bytes = image();
    pe::word(&mut bytes, 0x2080, 0x20b0);
    pe::word(&mut bytes, 0x2088, 0x20a0);
    assert_eq!(parse_module(&bytes).err(), Some(LoadError::Malformed));
}
