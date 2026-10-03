#[path = "support/pe32.rs"]
mod pe;

use ring3_engine::{
    loader::{ImageMetadata32, LoadError, load_pe32},
    memory::{Access, AddressSpace, FaultReason, GuestAddress, MemoryError, MemoryFault},
    process::{EngineInstance, HostError},
};

const KEY: u64 = 0xfedc_ba98_7654_3210;

fn metadata(actual: ImageMetadata32) {
    assert_eq!(actual.image_base, pe::BASE);
    assert_eq!(actual.image_size, pe::IMAGE_SIZE);
    assert_eq!(actual.entry_point, pe::BASE + 0x1000);
    assert_eq!(actual.mapped_pages, pe::MAPPED_PAGES);
}

fn read(memory: &AddressSpace, address: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    memory.read(GuestAddress(address), &mut bytes).unwrap();
    bytes
}

fn permissions(memory: &AddressSpace, address: u32, expected: [bool; 3]) {
    for (access, allowed) in [Access::Read, Access::Write, Access::Execute]
        .into_iter()
        .zip(expected)
    {
        let result = memory.resolve(GuestAddress(address), access);
        if allowed {
            assert!(result.is_ok(), "{address:#x} {access:?}: {result:?}");
        } else {
            assert_eq!(
                result,
                Err(MemoryError::Fault(MemoryFault {
                    address: GuestAddress(address),
                    access,
                    reason: FaultReason::Permission,
                }))
            );
        }
    }
}

#[test]
fn raw_padding_zero_fill_permissions_and_sparse_image_metadata_are_exact() {
    let bytes = pe::image();
    let (memory, info) = load_pe32(&bytes, 4).unwrap().into_parts();
    metadata(info);
    assert_eq!(memory.capacity_pages(), 4);
    assert_eq!(memory.mapped_pages(), 4);
    assert_eq!(read(&memory, pe::BASE, 512), bytes[..512]);
    assert_eq!(read(&memory, pe::BASE + 512, 3584), vec![0; 3584]);
    assert_eq!(
        read(&memory, pe::BASE + 0x1000, 512),
        bytes[pe::TEXT_RAW..pe::TEXT_RAW + 512]
    );
    assert_eq!(read(&memory, pe::BASE + 0x1200, 3584), vec![0; 3584]);
    assert_eq!(
        read(&memory, pe::BASE + 0x3000, 512),
        bytes[pe::DATA_RAW..pe::DATA_RAW + 512]
    );
    assert_eq!(read(&memory, pe::BASE + 0x3200, 3584), vec![0; 3584]);
    assert_eq!(read(&memory, pe::BASE + 0x5000, 4096), vec![0; 4096]);
    permissions(&memory, pe::BASE, [true, false, false]);
    permissions(&memory, pe::BASE + 0x1000, [true, false, true]);
    permissions(&memory, pe::BASE + 0x3000, [true, true, false]);
    permissions(&memory, pe::BASE + 0x5000, [true, true, false]);
    for rva in [0x2000, 0x4000, 0x6000, pe::IMAGE_SIZE] {
        for access in [Access::Read, Access::Write, Access::Execute] {
            let address = GuestAddress(pe::BASE + rva);
            assert_eq!(
                memory.resolve(address, access),
                Err(MemoryError::Fault(MemoryFault {
                    address,
                    access,
                    reason: FaultReason::Unmapped,
                }))
            );
        }
    }
}

#[test]
fn section_permission_bits_are_independent_and_page_span_uses_larger_size() {
    for bits in 0..8u32 {
        let mut bytes = pe::image();
        pe::put32(&mut bytes, pe::section(1) + 36, 0x40 | (bits << 29));
        let (memory, _) = load_pe32(&bytes, 4).unwrap().into_parts();
        permissions(
            &memory,
            pe::BASE + 0x3000,
            [bits & 2 != 0, bits & 4 != 0, bits & 1 != 0],
        );
    }
    let mut bytes = pe::image();
    pe::put32(&mut bytes, pe::section(1) + 8, 4097);
    let (memory, info) = load_pe32(&bytes, 5).unwrap().into_parts();
    assert_eq!(info.mapped_pages, 5);
    assert_eq!(read(&memory, pe::BASE + 0x4000, 4096), vec![0; 4096]);
    permissions(&memory, pe::BASE + 0x4000, [true, true, false]);
}

#[test]
fn optional_supported_coff_bits_gui_and_zero_dll_characteristics_are_accepted() {
    for coff in [0x0102, 0x0103, 0x0122, 0x0123] {
        let mut bytes = pe::image();
        pe::put16(&mut bytes, pe::COFF + 18, coff);
        pe::put16(&mut bytes, pe::OPTIONAL + 68, 2);
        pe::put16(&mut bytes, pe::OPTIONAL + 70, 0);
        metadata(load_pe32(&bytes, 4).unwrap().into_parts().1);
    }
}

fn rejected(bytes: &[u8], expected: LoadError, label: &str) {
    assert_eq!(load_pe32(bytes, 4).err(), Some(expected), "{label}");
}

#[test]
fn required_header_and_section_bytes_cannot_be_truncated() {
    let bytes = pe::image();
    for length in [
        0,
        1,
        2,
        0x3f,
        pe::PE + 3,
        pe::COFF + 19,
        pe::OPTIONAL + 223,
        pe::SECTIONS + 119,
        511,
        1023,
        bytes.len() - 1,
    ] {
        rejected(&bytes[..length], LoadError::Malformed, "truncated file");
    }
}

#[test]
fn malformed_offsets_bounds_alignment_overlap_and_entry_ranges_are_rejected() {
    let cases = [
        ("DOS magic", 0, 0),
        ("PE before DOS header", 0x3c, 0x20),
        ("PE offset overflow", 0x3c, u32::MAX),
        ("PE signature", pe::PE, 0),
        ("zero base", pe::OPTIONAL + 28, 0),
        ("unaligned base", pe::OPTIONAL + 28, pe::BASE + 1),
        ("zero image size", pe::OPTIONAL + 56, 0),
        ("unaligned image size", pe::OPTIONAL + 56, 0x7001),
        ("image too small", pe::OPTIONAL + 56, 0x5000),
        ("zero header size", pe::OPTIONAL + 60, 0),
        ("unaligned headers", pe::OPTIONAL + 60, 511),
        ("headers beyond file", pe::OPTIONAL + 60, 2048),
        ("zero virtual size", pe::section(0) + 8, 0),
        ("unaligned section RVA", pe::section(0) + 12, 0x1001),
        ("section overlaps headers", pe::section(0) + 12, 0),
        ("equal section RVA", pe::section(1) + 12, 0x1000),
        ("descending section RVA", pe::section(2) + 12, 0x2000),
        ("rounded page overlap", pe::section(1) + 8, 0x2001),
        ("section virtual overflow", pe::section(2) + 8, u32::MAX),
        ("raw size unaligned", pe::section(0) + 16, 513),
        ("raw pointer unaligned", pe::section(0) + 20, 513),
        ("raw overlaps headers", pe::section(0) + 20, 0),
        ("raw ranges overlap", pe::section(1) + 20, 512),
        ("raw pointer overflow", pe::section(1) + 20, 0xffff_fe00),
        ("zero raw size nonzero pointer", pe::section(2) + 20, 512),
        ("entry in headers", pe::OPTIONAL + 16, 1),
        ("entry in image gap", pe::OPTIONAL + 16, 0x2000),
        ("entry in raw padding", pe::OPTIONAL + 16, 0x1010),
        ("entry in pure BSS", pe::OPTIONAL + 16, 0x5000),
        ("entry outside image", pe::OPTIONAL + 16, 0x7000),
        ("entry not executable", pe::section(0) + 36, 0x4000_0020),
    ];
    for (label, offset, value) in cases {
        let mut bytes = pe::image();
        pe::put32(&mut bytes, offset, value);
        rejected(&bytes, LoadError::Malformed, label);
    }
    let mut bytes = pe::image();
    pe::put32(&mut bytes, pe::OPTIONAL + 28, 0xffff_0000);
    pe::put32(&mut bytes, pe::OPTIONAL + 56, 0x20000);
    rejected(&bytes, LoadError::Malformed, "base image overflow");
    let mut bytes = pe::image();
    pe::put32(&mut bytes, pe::section(0) + 20, pe::DATA_RAW as u32);
    pe::put32(&mut bytes, pe::section(1) + 20, pe::TEXT_RAW as u32);
    rejected(
        &bytes,
        LoadError::Malformed,
        "descending disjoint raw ranges",
    );
    let mut bytes = pe::image();
    pe::put16(&mut bytes, pe::COFF + 2, 0);
    rejected(&bytes, LoadError::Malformed, "zero sections");
    // raw remains valid; the entry lies in the virtual tail after initialized bytes.
    pe::put16(&mut bytes, pe::COFF + 2, 3);
    pe::put32(&mut bytes, pe::section(0) + 8, 1024);
    pe::put32(&mut bytes, pe::OPTIONAL + 16, 0x1200);
    rejected(&bytes, LoadError::Malformed, "entry in virtual tail");
    let mut bytes = pe::image();
    pe::put32(&mut bytes, 0x3c, 0x100);
    bytes.resize(0x800, 0);
    let header = pe::image();
    bytes[0x100..0x100 + (0x1f0 - pe::PE)].copy_from_slice(&header[pe::PE..0x1f0]);
    rejected(
        &bytes,
        LoadError::Malformed,
        "section table extends beyond declared headers",
    );
}

#[test]
fn unsupported_profiles_and_every_nonempty_directory_are_explicit_refusals() {
    for (label, offset, value) in [
        ("AMD64", pe::COFF, 0x8664),
        ("too many sections", pe::COFF + 2, 9),
        ("short optional header", pe::COFF + 16, 223),
        ("large optional header", pe::COFF + 16, 225),
        ("DLL", pe::COFF + 18, 0x2103),
        ("unknown COFF flag", pe::COFF + 18, 0x0143),
        ("missing executable flag", pe::COFF + 18, 0x0101),
        ("missing 32bit flag", pe::COFF + 18, 3),
        ("PE32+", pe::OPTIONAL, 0x20b),
        ("native subsystem", pe::OPTIONAL + 68, 1),
        ("dynamic base", pe::OPTIONAL + 70, 0x0140),
    ] {
        let mut bytes = pe::image();
        pe::put16(&mut bytes, offset, value);
        rejected(&bytes, LoadError::Unsupported, label);
    }
    for (label, offset, value) in [
        ("COFF symbols", pe::COFF + 8, 512),
        ("COFF symbol count", pe::COFF + 12, 1),
        ("section alignment", pe::OPTIONAL + 32, 8192),
        ("file alignment", pe::OPTIONAL + 36, 4096),
        ("reserved version", pe::OPTIONAL + 52, 1),
        ("loader flags", pe::OPTIONAL + 88, 1),
        ("directory count", pe::OPTIONAL + 92, 15),
        ("directory count overflow", pe::OPTIONAL + 92, u32::MAX),
        ("section relocations", pe::section(0) + 24, 512),
        ("section line numbers", pe::section(0) + 28, 512),
        ("section record counts", pe::section(0) + 32, 1),
        (
            "discardable section",
            pe::section(1) + 36,
            pe::RW | 0x0200_0000,
        ),
    ] {
        let mut bytes = pe::image();
        pe::put32(&mut bytes, offset, value);
        rejected(&bytes, LoadError::Unsupported, label);
    }
    for index in 0..16 {
        for field in [0, 4] {
            let mut bytes = pe::image();
            pe::put32(&mut bytes, pe::OPTIONAL + 96 + index * 8 + field, 1);
            rejected(&bytes, LoadError::Unsupported, "nonempty data directory");
        }
    }
}

#[test]
fn resident_and_file_image_resource_limits_fail_before_publication() {
    for pages in [0, 1, 2, 3, 4097, u32::MAX] {
        assert_eq!(
            load_pe32(&pe::image(), pages).err(),
            Some(LoadError::Capacity)
        );
    }
    let mut bytes = pe::image();
    bytes.resize(16 * 1024 * 1024 + 1, 0);
    rejected(&bytes, LoadError::Capacity, "oversized file");
    let mut bytes = pe::image();
    pe::put32(&mut bytes, pe::OPTIONAL + 56, 16 * 1024 * 1024 + 4096);
    rejected(&bytes, LoadError::Capacity, "oversized image");
}

fn sentinel(engine: &mut EngineInstance) {
    for (index, byte) in engine.arena_mut().unwrap().iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(31).wrapping_add(7);
    }
}

fn fresh_observation(engine: &EngineInstance) -> (Vec<u8>, usize, Vec<u8>, u32) {
    (
        engine.arena().to_vec(),
        engine.arena_address(),
        engine.dispatcher_bytes(KEY).unwrap().to_vec(),
        engine.memory().unwrap().capacity_pages(),
    )
}

#[test]
fn process_success_changes_only_guest_image_and_latches_loading_after_unmap() {
    let mut engine = EngineInstance::new(4, KEY).unwrap();
    sentinel(&mut engine);
    let before = fresh_observation(&engine);
    metadata(engine.load_pe32(&pe::image()).unwrap());
    assert_eq!(fresh_observation(&engine), before);
    assert_eq!(engine.key(), KEY);
    assert_eq!(engine.generation(), 0);
    assert!(engine.is_open());
    assert_eq!(engine.memory().unwrap().mapped_pages(), 4);
    let snapshot = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(pe::BASE + 0x1000), 5)
        .unwrap();
    assert_eq!(engine.load_pe32(&[]), Err(HostError::InvalidRequest));
    assert!(engine.memory().unwrap().is_code_current(&snapshot));
    assert_eq!(fresh_observation(&engine), before);
    for rva in [0, 0x1000, 0x3000, 0x5000] {
        engine.unmap(pe::BASE + rva, 1).unwrap();
    }
    assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
    assert_eq!(
        engine.load_pe32(&pe::image()),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(fresh_observation(&engine), before);
}

#[test]
fn failed_pristine_load_preserves_arena_dispatcher_capacity_and_accepts_later_valid_image() {
    let mut unsupported = pe::image();
    pe::put16(&mut unsupported, pe::COFF, 0x8664);
    for (bytes, expected, pages) in [
        (Vec::new(), LoadError::Malformed, 4),
        (unsupported, LoadError::Unsupported, 4),
        (pe::image(), LoadError::Capacity, 3),
    ] {
        let mut engine = EngineInstance::new(pages, KEY).unwrap();
        sentinel(&mut engine);
        let before = fresh_observation(&engine);
        assert_eq!(engine.load_pe32(&bytes), Err(HostError::Loader(expected)));
        assert_eq!(fresh_observation(&engine), before);
        assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
        assert_eq!(engine.key(), KEY);
        assert_eq!(engine.generation(), 0);
        assert!(engine.is_open());
        if pages == 4 {
            metadata(engine.load_pe32(&pe::image()).unwrap());
            assert_eq!(fresh_observation(&engine), before);
        }
    }
}

#[test]
fn mapped_or_compiled_instances_reject_load_without_changing_existing_code_identity() {
    for resident in [None, Some(false), Some(true)] {
        let mut engine = EngineInstance::new(4, KEY).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        engine.arena_mut().unwrap()[140..145].copy_from_slice(&[0xb8, 42, 0, 0, 0]);
        engine.upload(0x1000, 5).unwrap();
        let snapshot = engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(0x1000), 5)
            .unwrap();
        let mut unit = None;
        if let Some(resident) = resident {
            pe::put32(engine.arena_mut().unwrap(), 140, 0x1000);
            pe::put32(engine.arena_mut().unwrap(), 144, 5);
            if resident {
                unit = Some(engine.compile_resident(1).unwrap().get());
            } else {
                engine.compile(1).unwrap();
            }
        }
        let before = fresh_observation(&engine);
        let ram = read(engine.memory().unwrap(), 0x1000, 4096);
        let generation = engine.generation();
        let artifact = unit.map(|id| engine.resident_bytes(id).unwrap().to_vec());
        let valid = pe::image();
        for bytes in [&[][..], valid.as_slice()] {
            assert_eq!(engine.load_pe32(bytes), Err(HostError::InvalidRequest));
            assert_eq!(fresh_observation(&engine), before);
            assert_eq!(read(engine.memory().unwrap(), 0x1000, 4096), ram);
            assert!(engine.memory().unwrap().is_code_current(&snapshot));
            assert_eq!(engine.generation(), generation);
            if let Some(id) = unit {
                assert_eq!(
                    engine.resident_bytes(id).unwrap(),
                    artifact.as_ref().unwrap()
                );
            }
        }
        engine.unmap(0x1000, 1).unwrap();
        if resident.is_some() {
            assert_eq!(
                engine.load_pe32(&pe::image()),
                Err(HostError::InvalidRequest)
            );
            assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
            assert_eq!(fresh_observation(&engine), before);
        } else {
            // ordinary map/unmap history is not the successful-image latch.
            metadata(engine.load_pe32(&pe::image()).unwrap());
        }
    }
}

#[test]
fn closed_precedence_preserves_arena_and_never_reopens_instance() {
    let mut engine = EngineInstance::new(4, KEY).unwrap();
    sentinel(&mut engine);
    engine.close();
    let arena = engine.arena().to_vec();
    let pointer = engine.arena_address();
    let valid = pe::image();
    for bytes in [&[][..], valid.as_slice()] {
        assert_eq!(engine.load_pe32(bytes), Err(HostError::Closed));
        assert!(!engine.is_open());
        assert_eq!(engine.key(), KEY);
        assert_eq!(engine.generation(), 0);
        assert_eq!(engine.arena(), arena);
        assert_eq!(engine.arena_address(), pointer);
        assert!(matches!(engine.memory(), Err(HostError::Closed)));
    }
}

#[test]
fn maximum_image_ends_at_u32_limit_with_two_header_pages_and_exact_resident_capacity() {
    let bytes = pe::boundary_image();
    assert_eq!(load_pe32(&bytes, 4095).err(), Some(LoadError::Capacity));
    let (memory, info) = load_pe32(&bytes, 4096).unwrap().into_parts();
    assert_eq!(
        (
            info.image_base,
            info.image_size,
            info.entry_point,
            info.mapped_pages
        ),
        (0xff00_0000, 0x0100_0000, 0xff00_2000, 4096)
    );
    assert_eq!(
        u64::from(info.image_base) + u64::from(info.image_size),
        1 << 32
    );
    assert_eq!(memory.capacity_pages(), 4096);
    assert_eq!(memory.mapped_pages(), 4096);
    assert_eq!(read(&memory, 0xff00_1000, 512), bytes[0x1000..0x1200]);
    permissions(&memory, 0xff00_1000, [true, false, false]);
    assert_eq!(read(&memory, 0xff00_1200, 3584), vec![0; 3584]);
    permissions(&memory, 0xff00_1fff, [true, false, false]);
    assert_eq!(read(&memory, u32::MAX, 1), vec![0]);
    permissions(&memory, u32::MAX, [true, true, false]);
}
