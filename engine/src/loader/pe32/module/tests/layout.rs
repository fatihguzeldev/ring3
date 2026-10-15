use super::{support as pe, *};
use crate::memory::Permissions;

#[test]
fn general_dll_without_entry_and_small_alignment_preserve_shared_page_contents() {
    let bytes = pe::image(true, true);
    let image = parse_module(&bytes).unwrap();
    assert_eq!(image.kind, ImageKind32::Dll);
    assert_eq!(image.entry_rva, None);
    assert_eq!(image.mapped_pages, 1);
    let pages = image.prepare_pages(&bytes, pe::BASE).unwrap();
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].address.0, pe::BASE);
    assert_eq!(pages[0].permissions, Permissions::ALL);
    assert_eq!(&pages[0].bytes[..0x200], &bytes[..0x200]);
    assert_eq!(pages[0].bytes[0x200], 0xc3);
    assert_eq!(&pages[0].bytes[0x400..0x600], &[0x5a; 0x200]);
    assert!(pages[0].bytes[0x600..].iter().all(|byte| *byte == 0));
}

#[test]
fn mapped_zero_fill_and_final_section_permissions_do_not_depend_on_source_padding() {
    let bytes = pe::image(false, false);
    let image = parse_module(&bytes).unwrap();
    let pages = image.prepare_pages(&bytes, pe::BASE).unwrap();
    assert_eq!(pages.len(), 3);
    assert_eq!(pages[0].permissions, Permissions::READ);
    assert_eq!(pages[1].permissions, Permissions::READ_EXECUTE);
    assert_eq!(pages[2].permissions, Permissions::READ_WRITE);
    assert_eq!(&pages[2].bytes[..0x200], &[0x5a; 0x200]);
    assert!(pages[2].bytes[0x200..].iter().all(|byte| *byte == 0));
    assert_eq!(image.stack_reserve, 0x100000);
    assert_eq!(image.stack_commit, 0x1000);
}

#[test]
fn overlapping_logical_sections_and_inconsistent_small_alignment_fail_closed() {
    let mut bytes = pe::image(false, true);
    pe::put32(&mut bytes, pe::OPTIONAL + 224 + 40 + 12, 0x200);
    assert_eq!(parse_module(&bytes).err(), Some(LoadError::Malformed));
    let mut bytes = pe::image(false, true);
    pe::put32(&mut bytes, pe::OPTIONAL + 36, 0x100);
    assert_eq!(parse_module(&bytes).err(), Some(LoadError::Malformed));
}

#[test]
fn ordinary_larger_file_and_section_alignment_keep_sparse_page_permissions() {
    let mut bytes = pe::image(false, false);
    bytes.resize(0x3000, 0);
    pe::put32(&mut bytes, pe::OPTIONAL + 16, 0x2000);
    pe::put32(&mut bytes, pe::OPTIONAL + 32, 0x2000);
    pe::put32(&mut bytes, pe::OPTIONAL + 36, 0x1000);
    pe::put32(&mut bytes, pe::OPTIONAL + 56, 0x6000);
    pe::put32(&mut bytes, pe::OPTIONAL + 60, 0x1000);
    for (index, rva, raw, size) in [(0, 0x2000, 0x1000, 0x1000), (1, 0x4000, 0x2000, 0x1800)] {
        let at = pe::OPTIONAL + 224 + index * 40;
        pe::put32(&mut bytes, at + 8, size);
        pe::put32(&mut bytes, at + 12, rva);
        pe::put32(&mut bytes, at + 16, 0x1000);
        pe::put32(&mut bytes, at + 20, raw);
    }
    bytes[0x1000] = 0xc3;
    bytes[0x2000..0x3000].fill(0xa5);
    let image = parse_module(&bytes).unwrap();
    assert_eq!(image.mapped_pages, 4);
    let pages = image.prepare_pages(&bytes, pe::BASE).unwrap();
    assert_eq!(
        pages
            .iter()
            .map(|page| page.address.0 - pe::BASE)
            .collect::<Vec<_>>(),
        vec![0, 0x2000, 0x4000, 0x5000]
    );
    assert_eq!(pages[1].permissions, Permissions::READ_EXECUTE);
    assert!(pages[3].bytes.iter().all(|byte| *byte == 0));
}

#[test]
fn general_section_table_is_not_limited_to_legacy_eight_sections() {
    let mut bytes = pe::image(false, false);
    bytes.resize(0x1600, 0);
    pe::put16(&mut bytes, pe::COFF + 2, 9);
    pe::put32(&mut bytes, pe::OPTIONAL + 56, 0xa000);
    pe::put32(&mut bytes, pe::OPTIONAL + 60, 0x400);
    bytes[pe::OPTIONAL + 224..pe::OPTIONAL + 224 + 9 * 40].fill(0);
    for index in 0..9 {
        let at = pe::OPTIONAL + 224 + index * 40;
        pe::put32(&mut bytes, at + 8, 1);
        pe::put32(&mut bytes, at + 12, (index as u32 + 1) * 4096);
        pe::put32(&mut bytes, at + 16, 512);
        pe::put32(&mut bytes, at + 20, 0x400 + index as u32 * 512);
        pe::put32(&mut bytes, at + 36, 0x6000_0020);
    }
    let image = parse_module(&bytes).unwrap();
    assert_eq!(image.sections.len(), 9);
    assert_eq!(image.mapped_pages, 10);
}
