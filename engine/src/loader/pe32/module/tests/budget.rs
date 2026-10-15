use super::{support as pe, *};

fn limits(entries: usize, strings: usize) -> ParserLimits32 {
    ParserLimits32 {
        metadata_entries: entries,
        string_bytes: strings,
    }
}

#[test]
fn section_and_string_budgets_have_exact_cumulative_boundaries() {
    let bytes = pe::empty_data();
    assert!(parse_module_with_limits(&bytes, limits(2, 0)).is_ok());
    assert_eq!(
        parse_module_with_limits(&bytes, limits(1, 0)).err(),
        Some(LoadError::Capacity)
    );
    let mut bytes = bytes;
    pe::directory(&mut bytes, 1, 0x2000, 40);
    pe::word(&mut bytes, 0x2000, 0x2080);
    pe::word(&mut bytes, 0x200c, 0x2060);
    pe::word(&mut bytes, 0x2010, 0x20a0);
    pe::data(&mut bytes, 0x2060, b"first.dll\0");
    pe::word(&mut bytes, 0x2080, 0x8000_0001);
    assert!(parse_module_with_limits(&bytes, limits(4, 9)).is_ok());
    assert_eq!(
        parse_module_with_limits(&bytes, limits(3, 9)).err(),
        Some(LoadError::Capacity)
    );
    assert_eq!(
        parse_module_with_limits(&bytes, limits(4, 8)).err(),
        Some(LoadError::Capacity)
    );
}

#[test]
fn zero_filled_export_counts_are_bounded_before_reserving_table_storage() {
    let mut bytes = pe::empty_data();
    pe::put32(&mut bytes, pe::OPTIONAL + 56, 0x4000_0000);
    pe::put32(&mut bytes, pe::OPTIONAL + 224 + 40 + 8, 0x3fff_e000);
    pe::directory(&mut bytes, 0, 0x2000, 40);
    pe::word(&mut bytes, 0x2014, 0x0800_0000);
    pe::word(&mut bytes, 0x201c, 0x3000);
    assert_eq!(parse_module(&bytes).err(), Some(LoadError::Capacity));
}

#[test]
fn relocation_padding_and_tls_callbacks_share_the_metadata_budget() {
    let mut bytes = pe::empty_data();
    pe::directory(&mut bytes, 5, 0x2100, 12);
    pe::word(&mut bytes, 0x2100, 0x1000);
    pe::word(&mut bytes, 0x2104, 12);
    assert!(parse_module_with_limits(&bytes, limits(5, 0)).is_ok());
    assert_eq!(
        parse_module_with_limits(&bytes, limits(4, 0)).err(),
        Some(LoadError::Capacity)
    );
    pe::directory(&mut bytes, 9, 0x2000, 24);
    pe::word(&mut bytes, 0x2008, pe::BASE + 0x2200);
    pe::word(&mut bytes, 0x200c, pe::BASE + 0x2080);
    pe::word(&mut bytes, 0x2080, pe::BASE + 0x1000);
    assert!(parse_module_with_limits(&bytes, limits(6, 0)).is_ok());
    assert_eq!(
        parse_module_with_limits(&bytes, limits(5, 0)).err(),
        Some(LoadError::Capacity)
    );
}
