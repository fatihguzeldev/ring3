use super::{support as pe, *};

#[test]
fn variable_optional_header_and_directory_count_locate_the_actual_section_table() {
    let mut bytes = pe::image(true, false);
    let sections = bytes[pe::OPTIONAL + 224..pe::OPTIONAL + 224 + 80].to_vec();
    pe::put16(&mut bytes, pe::COFF + 16, 112);
    pe::put32(&mut bytes, pe::OPTIONAL + 92, 2);
    bytes[pe::OPTIONAL + 112..pe::OPTIONAL + 112 + 80].copy_from_slice(&sections);
    assert_eq!(parse_module(&bytes).unwrap().sections.len(), 2);
    pe::put32(&mut bytes, pe::OPTIONAL + 92, 3);
    assert_eq!(parse_module(&bytes).err(), Some(LoadError::Malformed));
}

#[test]
fn passive_resource_and_file_offset_certificate_are_not_import_dependencies() {
    let mut bytes = pe::empty_data();
    pe::directory(&mut bytes, 2, 0x2080, 16);
    bytes.resize(0x810, 0);
    pe::directory(&mut bytes, 4, 0x800, 16);
    pe::put32(&mut bytes, 0x800, 16);
    pe::put16(&mut bytes, 0x804, 0x200);
    pe::put16(&mut bytes, 0x806, 2);
    let image = parse_module(&bytes).unwrap();
    assert!(image.imports.is_empty());
    assert_eq!(image.prepare_pages(&bytes, pe::BASE).unwrap().len(), 3);
    pe::directory(&mut bytes, 4, 0x2000, 16);
    assert_eq!(parse_module(&bytes).err(), Some(LoadError::Malformed));
}

#[test]
fn delay_import_descriptor_stays_guest_owned_and_unsupported_runtime_kinds_fail_closed() {
    let mut bytes = pe::empty_data();
    pe::directory(&mut bytes, 13, 0x2000, 64);
    pe::word(&mut bytes, 0x2000, 1);
    pe::word(&mut bytes, 0x2004, 0x2080);
    pe::word(&mut bytes, 0x2008, 0x2200);
    pe::word(&mut bytes, 0x200c, 0x20a0);
    pe::word(&mut bytes, 0x2010, 0x20c0);
    pe::data(&mut bytes, 0x2080, b"late.dll\0");
    let image = parse_module(&bytes).unwrap();
    assert_eq!(
        image.delay_import_directory,
        Some(Directory32 {
            rva: 0x2000,
            size: 64
        })
    );
    assert!(image.imports.is_empty());
    let pages = image.prepare_pages(&bytes, pe::BASE).unwrap();
    assert_eq!(&pages[2].bytes[..64], &bytes[0x400..0x440]);
    pe::directory(&mut bytes, 14, 0x2080, 16);
    assert_eq!(parse_module(&bytes).err(), Some(LoadError::Unsupported));
}
