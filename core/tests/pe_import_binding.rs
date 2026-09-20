#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::PeImportSymbol;
use ring3_core::execution::{LoadError, load_pe32, load_pe32_with_imports};

#[test]
fn resolves_named_and_ordinal_imports_before_final_protection() {
    let mut bytes = imported_executable::pe32(&[0xcc], "Example.dll", &["Read", "Write"]);
    bytes[1092..1096].copy_from_slice(&0x8000_0007_u32.to_le_bytes());
    bytes[0x1c4..0x1c8].copy_from_slice(&0x4000_0040_u32.to_le_bytes());
    let mut calls = 0;
    let mut image = load_pe32_with_imports(&bytes, 3, |module, symbol| {
        assert_eq!(module, "Example.dll");
        calls += 1;
        match symbol {
            PeImportSymbol::ByName { name: "Read", .. } => Some(0x7000_0000),
            PeImportSymbol::Ordinal(7) => Some(0x7000_0010),
            _ => None,
        }
    })
    .unwrap();
    assert_eq!(calls, 2);
    let mut slots = [0; 12];
    image.memory.read(0x0040_2060, &mut slots).unwrap();
    assert_eq!(&slots[..4], &0x7000_0000_u32.to_le_bytes());
    assert_eq!(&slots[4..8], &0x7000_0010_u32.to_le_bytes());
    assert_eq!(&slots[8..], &[0; 4]);
    assert!(image.memory.write(0x0040_2060, &[0]).is_err());
    assert!(load_pe32(&bytes, 3).is_err());
}

#[test]
fn unresolved_and_bad_iat_imports_fail_loading() {
    let bytes = imported_executable::pe32(&[0xcc], "Missing.dll", &["Absent"]);
    assert!(matches!(
        load_pe32_with_imports(&bytes, 3, |_, _| None),
        Err(LoadError::UnresolvedImport { .. })
    ));
    for rva in [0_u32, 0x100, 0x2fff, 0xffff_fffc] {
        let mut bad = bytes.clone();
        bad[1040..1044].copy_from_slice(&rva.to_le_bytes());
        assert!(load_pe32_with_imports(&bad, 3, |_, _| Some(0x7000_0000)).is_err());
    }
}

#[test]
fn overlapping_iat_tables_are_rejected() {
    let mut bytes = imported_executable::pe32(&[0xcc], "Example.dll", &["Read"]);
    bytes[0x104..0x108].copy_from_slice(&60_u32.to_le_bytes());
    bytes.copy_within(1024..1044, 1044);
    assert!(matches!(
        load_pe32_with_imports(&bytes, 3, |_, _| Some(0x7000_0000)),
        Err(LoadError::InvalidImportAddressTable)
    ));
}

#[test]
fn malformed_and_bound_imports_are_not_silently_loaded() {
    let mut bytes = imported_executable::pe32(&[0xcc], "Example.dll", &["Read"]);
    bytes[1028..1032].copy_from_slice(&1_u32.to_le_bytes());
    assert!(load_pe32_with_imports(&bytes, 3, |_, _| Some(0x7000_0000)).is_err());
    bytes[1028..1032].fill(0);
    bytes[1024..1028].fill(0);
    assert!(load_pe32_with_imports(&bytes, 3, |_, _| Some(0x7000_0000)).is_err());
}
