#[allow(dead_code)]
#[path = "support/pe32.rs"]
mod pe;

#[path = "support/pe32_relocation.rs"]
mod reloc;

use ring3_engine::{
    loader::{LoadError, load_pe32, load_pe32_at},
    memory::{Access, AddressSpace, FaultReason, GuestAddress, MemoryError, MemoryFault},
    process::{EngineInstance, HostError},
};

const KEY: u64 = 0xfedc_ba98_7654_3210;

fn read(memory: &AddressSpace, address: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    memory.read(GuestAddress(address), &mut bytes).unwrap();
    bytes
}

fn word(memory: &AddressSpace, address: u32) -> u32 {
    u32::from_le_bytes(read(memory, address, 4).try_into().unwrap())
}

fn permissions(memory: &AddressSpace, address: u32, expected: [bool; 3]) {
    for (access, allowed) in [Access::Read, Access::Write, Access::Execute]
        .into_iter()
        .zip(expected)
    {
        if allowed {
            assert!(memory.resolve(GuestAddress(address), access).is_ok());
        } else {
            assert_eq!(
                memory.resolve(GuestAddress(address), access),
                Err(MemoryError::Fault(MemoryFault {
                    address: GuestAddress(address),
                    access,
                    reason: FaultReason::Permission,
                }))
            );
        }
    }
}

fn rejection(bytes: &[u8], actual: u32, expected: LoadError, label: &str) {
    assert_eq!(
        load_pe32_at(bytes, actual, 32).err(),
        Some(expected),
        "{label}"
    );
}

#[test]
fn additive_api_loads_an_empty_directory_at_the_preferred_base() {
    let bytes = pe::image();
    let (_, info) = load_pe32_at(&bytes, pe::BASE, 4).unwrap().into_parts();
    assert_eq!(info.image_base, pe::BASE);
    assert_eq!(info.entry_point, pe::BASE + 0x1000);
    let mut engine = EngineInstance::new(4, 7).unwrap();
    assert_eq!(engine.load_pe32_at(&bytes, pe::BASE).unwrap(), info);
}

#[test]
fn upward_downward_and_zero_deltas_match_authored_words_and_final_permissions() {
    let bytes = reloc::full_image();
    let original = bytes.clone();
    for (actual, expected) in [
        (
            0x0040_0000,
            [0x0040_3000, 0xffff_fffc, 0x0000_3000, 0x0000_0002],
        ),
        (
            0x0050_0000,
            [0x0050_3000, 0x000f_fffc, 0x0010_3000, 0x0010_0002],
        ),
        (
            0x0030_0000,
            [0x0030_3000, 0xffef_fffc, 0xfff0_3000, 0xfff0_0002],
        ),
    ] {
        let (memory, info) = load_pe32_at(&bytes, actual, 6).unwrap().into_parts();
        assert_eq!(
            (
                info.image_base,
                info.image_size,
                info.entry_point,
                info.mapped_pages
            ),
            (actual, 0x9000, actual + 0x1000, 6)
        );
        assert_eq!(read(&memory, actual, 512), original[..512]);
        for ((rva, _), value) in reloc::WORDS.into_iter().zip(expected) {
            assert_eq!(
                word(&memory, actual + rva),
                value,
                "base {actual:#x} RVA {rva:#x}"
            );
        }
        assert_eq!(
            read(&memory, actual + reloc::DIRECTORY_RVA, 36),
            original[reloc::DIRECTORY_OFFSET..reloc::DIRECTORY_OFFSET + 36]
        );
        assert_eq!(read(&memory, actual + 0x7000, 4096), vec![0; 4096]);
        permissions(&memory, actual, [true, false, false]);
        permissions(&memory, actual + 0x1001, [true, false, true]);
        permissions(&memory, actual + 0x3ffe, [true, true, false]);
        assert_eq!(bytes, original);
    }
}

#[test]
fn final_ten_byte_block_and_absolute_padding_do_not_require_target_alignment_or_order() {
    let bytes = reloc::image(&reloc::directory(&[(0x1000, &[0x3001])]));
    let (memory, _) = load_pe32_at(&bytes, 0x0050_0000, 6).unwrap().into_parts();
    assert_eq!(word(&memory, 0x0050_1001), 0x0050_3000);
    let bytes = reloc::image(&reloc::directory(&[(0x8000, &[0x0fff])]));
    let (memory, _) = load_pe32_at(&bytes, 0x0050_0000, 6).unwrap().into_parts();
    for (rva, value) in reloc::WORDS {
        assert_eq!(word(&memory, 0x0050_0000 + rva), value);
    }
    let mut bytes = reloc::image(&reloc::directory(&[(0x5000, &[0x3ffc])]));
    pe::put32(&mut bytes, pe::DATA_RAW + 0x2ffc, 0xffff_ffff);
    let (memory, _) = load_pe32_at(&bytes, 0x0050_0000, 6).unwrap().into_parts();
    assert_eq!(word(&memory, 0x0050_5ffc), 0x000f_ffff);
}

#[test]
fn absent_stripped_and_legacy_directory_policies_are_preserved() {
    let plain = pe::image();
    rejection(
        &plain,
        0x0050_0000,
        LoadError::Unsupported,
        "rebasing without directory",
    );
    let bytes = reloc::full_image();
    assert_eq!(load_pe32(&bytes, 6).err(), Some(LoadError::Unsupported));
    let mut legacy = EngineInstance::new(6, KEY).unwrap();
    let arena = legacy.arena().to_vec();
    assert_eq!(
        legacy.load_pe32(&bytes),
        Err(HostError::Loader(LoadError::Unsupported))
    );
    assert_eq!(legacy.arena(), arena);
    let mut stripped = bytes.clone();
    pe::put16(&mut stripped, pe::COFF + 18, 0x0103);
    for actual in [pe::BASE, 0x0050_0000] {
        rejection(
            &stripped,
            actual,
            LoadError::Malformed,
            "directory with stripped flag",
        );
    }
    for index in (0..16).filter(|index| *index != 5) {
        let mut bytes = reloc::full_image();
        pe::put32(&mut bytes, pe::OPTIONAL + 96 + index * 8, 1);
        rejection(&bytes, pe::BASE, LoadError::Unsupported, "other directory");
    }
}

#[test]
fn malformed_directory_shape_and_block_consumption_are_rejected_even_at_zero_delta() {
    for (label, field, value) in [
        ("only size", reloc::DIRECTORY_FIELD, 0),
        ("only RVA", reloc::DIRECTORY_FIELD + 4, 0),
        ("misaligned directory", reloc::DIRECTORY_FIELD, 0x5001),
        ("directory in headers", reloc::DIRECTORY_FIELD, 0x100),
        ("directory in gap", reloc::DIRECTORY_FIELD, 0x2000),
        ("directory outside image", reloc::DIRECTORY_FIELD, 0x9000),
        (
            "directory crosses initialized end",
            reloc::DIRECTORY_FIELD,
            0x5ffc,
        ),
    ] {
        let mut bytes = reloc::full_image();
        pe::put32(&mut bytes, field, value);
        rejection(&bytes, pe::BASE, LoadError::Malformed, label);
    }
    for (label, field, value) in [
        ("unaligned page", 0, 0x1001),
        ("page outside image", 0, 0x9000),
        ("page RVA overflow", 0, 0xffff_f000),
        ("zero block size", 4, 0),
        ("short block size", 4, 6),
        ("odd block size", 4, 9),
        ("block exceeds directory", 4, 64),
    ] {
        let mut bytes = reloc::image(&reloc::directory(&[(0x1000, &[0x3001, 0])]));
        pe::put32(&mut bytes, reloc::DIRECTORY_OFFSET + field, value);
        rejection(&bytes, pe::BASE, LoadError::Malformed, label);
    }
    for (label, directory) in [
        ("truncated block header", vec![0; 6]),
        (
            "misaligned next block",
            reloc::directory(&[(0x1000, &[0x3001]), (0x3000, &[0x3003])]),
        ),
        ("trailing bytes", {
            let mut bytes = reloc::directory(&[(0x1000, &[0x3001, 0])]);
            bytes.extend_from_slice(&[0; 2]);
            bytes
        }),
        ("zero sentinel block", {
            let mut bytes = reloc::directory(&[(0x1000, &[0x3001, 0])]);
            bytes.extend_from_slice(&[0; 8]);
            bytes
        }),
    ] {
        rejection(
            &reloc::image(&directory),
            pe::BASE,
            LoadError::Malformed,
            label,
        );
    }
}

#[test]
fn unsupported_fixup_types_are_not_hidden_by_zero_delta() {
    for kind in (1..16u16).filter(|kind| *kind != 3) {
        let bytes = reloc::image(&reloc::directory(&[(0x1000, &[(kind << 12) | 1, 0])]));
        rejection(
            &bytes,
            pe::BASE,
            LoadError::Unsupported,
            "unsupported relocation type",
        );
    }
}

#[test]
fn target_intersections_directory_aliases_and_overlapping_fixups_are_rejected() {
    for (label, page, entry) in [
        ("header", 0, 0x3100),
        ("gap", 0x2000, 0x3000),
        ("BSS", 0x7000, 0x3000),
        ("code raw padding", 0x1000, 0x3010),
        ("partial code end", 0x1000, 0x300e),
        ("partial section end", 0x5000, 0x3ffe),
        ("image end", 0x8000, 0x3ffe),
        ("directory overlap", 0x5000, 0x3001),
    ] {
        let bytes = reloc::image(&reloc::directory(&[(page, &[entry, 0])]));
        rejection(&bytes, pe::BASE, LoadError::Malformed, label);
    }
    for (label, virtual_size, raw_size) in [
        ("virtual padding", 0x2800, 0x3000),
        ("uninitialized tail", 0x3000, 0x2800),
    ] {
        let mut bytes = reloc::image(&reloc::directory(&[(0x5000, &[0x3900, 0])]));
        pe::put32(&mut bytes, pe::section(1) + 8, virtual_size);
        pe::put32(&mut bytes, pe::section(1) + 16, raw_size);
        rejection(&bytes, pe::BASE, LoadError::Malformed, label);
    }
    for (label, entries) in [
        ("duplicate targets", &[0x3003, 0x3003][..]),
        ("partial overlap", &[0x3003, 0x3005][..]),
    ] {
        let bytes = reloc::image(&reloc::directory(&[(0x3000, entries)]));
        rejection(&bytes, pe::BASE, LoadError::Malformed, label);
    }
}

#[test]
fn selected_base_and_existing_resident_limits_remain_checked() {
    let bytes = reloc::full_image();
    for actual in [0, 1, pe::BASE + 4096] {
        rejection(
            &bytes,
            actual,
            LoadError::Malformed,
            "invalid selected base",
        );
    }
    let mut overflow = bytes.clone();
    pe::put32(&mut overflow, pe::OPTIONAL + 56, 0x20000);
    rejection(
        &overflow,
        0xffff_0000,
        LoadError::Malformed,
        "selected image overflow",
    );
    for pages in [0, 5, 4097, u32::MAX] {
        assert_eq!(
            load_pe32_at(&bytes, pe::BASE, pages).err(),
            Some(LoadError::Capacity)
        );
    }
}

#[test]
fn directory_block_and_entry_caps_include_absolute_padding() {
    let max_entries = [0x0fff; 4];
    let max_blocks = vec![(0, &max_entries[..]); 1024];
    let bytes = reloc::image(&reloc::directory(&max_blocks));
    let (_, info) = load_pe32_at(&bytes, 0x0050_0000, 32).unwrap().into_parts();
    assert_eq!(info.image_base, 0x0050_0000);
    let excess_blocks = vec![(0, &[][..]); 1025];
    rejection(
        &reloc::image(&reloc::directory(&excess_blocks)),
        pe::BASE,
        LoadError::Capacity,
        "block cap",
    );
    let excess_entries = vec![0x0fff; 4097];
    rejection(
        &reloc::image(&reloc::directory(&[(0, &excess_entries)])),
        pe::BASE,
        LoadError::Capacity,
        "entry cap includes ABS",
    );
    rejection(
        &reloc::image(&vec![0; 16 * 1024 + 4]),
        pe::BASE,
        LoadError::Capacity,
        "directory byte cap before decoding",
    );
}

fn observe(engine: &EngineInstance) -> (Vec<u8>, usize, Vec<u8>, u32, u32, u64) {
    (
        engine.arena().to_vec(),
        engine.arena_address(),
        engine.dispatcher_bytes(KEY).unwrap().to_vec(),
        engine.generation(),
        engine.memory().unwrap().mapped_pages(),
        engine.key(),
    )
}

fn sentinel(engine: &mut EngineInstance) {
    for (index, byte) in engine.arena_mut().unwrap().iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(31).wrapping_add(7);
    }
}

#[test]
fn process_failures_keep_all_public_state_and_do_not_latch_pristine_instances() {
    let invalid = reloc::image(&reloc::directory(&[(0x1000, &[0x1001, 0])]));
    let malformed = reloc::image(&[0; 6]);
    let capacity = reloc::image(&vec![0; 16 * 1024 + 4]);
    for (bytes, expected) in [
        (malformed, LoadError::Malformed),
        (invalid, LoadError::Unsupported),
        (capacity, LoadError::Capacity),
    ] {
        let mut engine = EngineInstance::new(6, KEY).unwrap();
        sentinel(&mut engine);
        let before = observe(&engine);
        assert_eq!(
            engine.load_pe32_at(&bytes, 0x0050_0000),
            Err(HostError::Loader(expected))
        );
        assert_eq!(observe(&engine), before);
        assert!(engine.is_open());
        let good = reloc::full_image();
        let original = good.clone();
        let image = engine.load_pe32_at(&good, 0x0050_0000).unwrap();
        assert_eq!(image.image_base, 0x0050_0000);
        assert_eq!(engine.arena(), before.0);
        assert_eq!(engine.dispatcher_bytes(KEY).unwrap(), before.2);
        assert_eq!(engine.generation(), 0);
        assert_eq!(engine.key(), KEY);
        assert_eq!(good, original);
    }
}

#[test]
fn nonpristine_and_successful_image_latch_preserve_code_identity_and_closed_wins() {
    let mut mapped = EngineInstance::new(6, KEY).unwrap();
    mapped.map(0x1000, 1, 7).unwrap();
    mapped.arena_mut().unwrap()[140..145].copy_from_slice(&[0xb8, 42, 0, 0, 0]);
    mapped.upload(0x1000, 5).unwrap();
    let snapshot = mapped
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(0x1000), 5)
        .unwrap();
    let before = observe(&mapped);
    let ram = read(mapped.memory().unwrap(), 0x1000, 4096);
    assert_eq!(mapped.load_pe32_at(&[], 0), Err(HostError::InvalidRequest));
    assert_eq!(observe(&mapped), before);
    assert!(mapped.memory().unwrap().is_code_current(&snapshot));
    assert_eq!(read(mapped.memory().unwrap(), 0x1000, 4096), ram);

    let mut engine = EngineInstance::new(6, KEY).unwrap();
    sentinel(&mut engine);
    engine
        .load_pe32_at(&reloc::full_image(), 0x0050_0000)
        .unwrap();
    let snapshot = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(0x0050_1000), 16)
        .unwrap();
    let before = observe(&engine);
    assert_eq!(engine.load_pe32_at(&[], 0), Err(HostError::InvalidRequest));
    assert_eq!(observe(&engine), before);
    assert!(engine.memory().unwrap().is_code_current(&snapshot));
    for rva in [0, 0x1000, 0x3000, 0x4000, 0x5000, 0x7000] {
        engine.unmap(0x0050_0000 + rva, 1).unwrap();
    }
    assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
    assert_eq!(engine.load_pe32_at(&[], 0), Err(HostError::InvalidRequest));
    engine.close();
    let arena = engine.arena().to_vec();
    assert_eq!(engine.load_pe32_at(&[], 0), Err(HostError::Closed));
    assert_eq!(engine.arena(), arena);
    assert!(!engine.is_open());
}
