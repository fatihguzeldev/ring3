use super::{support as pe, *};

#[test]
fn static_tls_keeps_va_geometry_bss_index_callback_location_and_alignment() {
    let mut bytes = pe::empty_data();
    pe::directory(&mut bytes, 9, 0x2000, 24);
    for (offset, value) in [
        (0, pe::BASE + 0x2080),
        (4, pe::BASE + 0x2084),
        (8, pe::BASE + 0x2200),
        (12, pe::BASE + 0x20a0),
        (16, 5),
        (20, 0x0030_0000),
    ] {
        pe::word(&mut bytes, 0x2000 + offset, value);
    }
    pe::data(&mut bytes, 0x2080, &[1, 2, 3, 4]);
    pe::word(&mut bytes, 0x20a0, pe::BASE + 0x1000);
    let image = parse_module(&bytes).unwrap();
    assert_eq!(
        image.tls,
        Some(StaticTls32 {
            template_rva: 0x2080,
            template_size: 4,
            zero_fill: 5,
            index_rva: 0x2200,
            callbacks: vec![0x1000],
            callbacks_rva: Some(0x20a0),
            alignment: 4,
        })
    );
    let pages = image.prepare_pages(&bytes, pe::BASE).unwrap();
    assert_eq!(&pages[2].bytes[0x80..0x84], &[1, 2, 3, 4]);
    assert_eq!(&pages[2].bytes[0x200..0x204], &[0; 4]);
    let mut bad = bytes.clone();
    pe::word(&mut bad, 0x20a0, pe::BASE + 0x2200);
    assert_eq!(parse_module(&bad).err(), Some(LoadError::Malformed));
    pe::word(&mut bytes, 0x2004, pe::BASE + 0x207f);
    assert_eq!(parse_module(&bytes).err(), Some(LoadError::Malformed));
}

#[test]
fn load_config_exposes_cookie_safeseh_and_guard_policy_without_emulating_them() {
    let mut bytes = pe::empty_data();
    pe::directory(&mut bytes, 10, 0x2000, 92);
    pe::word(&mut bytes, 0x2000, 92);
    pe::word(&mut bytes, 0x203c, pe::BASE + 0x2200);
    pe::word(&mut bytes, 0x2040, pe::BASE + 0x2080);
    pe::word(&mut bytes, 0x2044, 1);
    pe::word(&mut bytes, 0x2058, 0x100);
    pe::word(&mut bytes, 0x2080, 0x1000);
    let image = parse_module(&bytes).unwrap();
    let config = image.load_config.unwrap();
    assert_eq!(config.security_cookie_rva, Some(0x2200));
    assert_eq!(config.seh_handlers, vec![0x1000]);
    assert_eq!(config.guard_flags, Some(0x100));
    pe::word(&mut bytes, 0x2000, 96);
    assert_eq!(parse_module(&bytes).err(), Some(LoadError::Malformed));
}
