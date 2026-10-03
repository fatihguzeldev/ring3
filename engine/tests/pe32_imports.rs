use ring3_engine::{
    cpu::dbt::GateSpec,
    loader::{self, ImageMetadata32, LinkedImageMetadata32, LoadError},
    memory::{Access, AddressSpace, FaultReason, GuestAddress, MemoryError, MemoryFault},
    process::{EngineInstance, HostError},
};

const KEY: u64 = 0xf123_4567_89ab_cdef;
const BASE: u32 = 0x0040_0000;
const GATE: u32 = 0x7000_0000;
const IMAGE_SIZE: u32 = 0x7000;
const COFF: usize = 0x84;
const OPTIONAL: usize = 0x98;
const SECTIONS: usize = 0x178;
const TEXT_RAW: usize = 0x200;
const DATA_RAW: usize = 0x400;
const DIRECTORY: u32 = 0x3000;
const ILT: u32 = 0x3040;
const IAT: u32 = 0x3080;
const MODULE: u32 = 0x30c0;
const GET_NAME: u32 = 0x3100;
const SET_NAME: u32 = 0x3110;
const RELOCATION: u32 = 0x3200;
const GET_ID: u32 = 0x0001_0001;
const SET_ID: u32 = 0x0001_0002;

fn put16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn file(rva: u32) -> usize {
    if rva < 0x3000 {
        TEXT_RAW + (rva - 0x1000) as usize
    } else {
        DATA_RAW + (rva - 0x3000) as usize
    }
}

fn directory(bytes: &mut [u8], index: usize, rva: u32, size: u32) {
    put32(bytes, OPTIONAL + 96 + index * 8, rva);
    put32(bytes, OPTIONAL + 100 + index * 8, size);
}

fn descriptor(bytes: &mut [u8], field: usize, value: u32) {
    put32(bytes, file(DIRECTORY) + field, value);
}

fn thunk(bytes: &mut [u8], index: usize, value: u32) {
    put32(bytes, file(ILT) + index * 4, value);
    put32(bytes, file(IAT) + index * 4, value);
}

fn section(
    bytes: &mut [u8],
    index: usize,
    rva: u32,
    virtual_size: u32,
    raw: u32,
    raw_size: u32,
    flags: u32,
) {
    let at = SECTIONS + index * 40;
    bytes[at..at + 8].copy_from_slice(b"authored");
    put32(bytes, at + 8, virtual_size);
    put32(bytes, at + 12, rva);
    put32(bytes, at + 16, raw_size);
    put32(bytes, at + 20, raw);
    put32(bytes, at + 36, flags);
}

fn image(names: &[u32]) -> Vec<u8> {
    // literal authored pe, independent of product parser/linker helpers and actual-wasm fixtures.
    let mut bytes = vec![0; 0x800];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 0x3c, 0x80);
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    put16(&mut bytes, COFF, 0x14c);
    put16(&mut bytes, COFF + 2, 3);
    put16(&mut bytes, COFF + 16, 224);
    put16(&mut bytes, COFF + 18, 0x0102);
    put16(&mut bytes, OPTIONAL, 0x010b);
    put32(&mut bytes, OPTIONAL + 16, 0x1000);
    put32(&mut bytes, OPTIONAL + 28, BASE);
    put32(&mut bytes, OPTIONAL + 32, 4096);
    put32(&mut bytes, OPTIONAL + 36, 512);
    put32(&mut bytes, OPTIONAL + 56, IMAGE_SIZE);
    put32(&mut bytes, OPTIONAL + 60, 512);
    put16(&mut bytes, OPTIONAL + 68, 3);
    put32(&mut bytes, OPTIONAL + 92, 16);
    directory(&mut bytes, 1, DIRECTORY, 40);
    directory(&mut bytes, 12, IAT, 4 * (names.len() as u32 + 1));
    section(&mut bytes, 0, 0x1000, 0x200, 0x200, 0x200, 0x6000_0020);
    section(&mut bytes, 1, 0x3000, 0x280, 0x400, 0x400, 0x4000_0040);
    section(&mut bytes, 2, 0x5000, 0x1000, 0, 0, 0xc000_0080);
    bytes[TEXT_RAW..TEXT_RAW + 0x200].fill(0x90);
    bytes[TEXT_RAW..TEXT_RAW + 6].copy_from_slice(&[0xb8, 42, 0, 0, 0, 0xc3]);
    put32(&mut bytes, file(0x1021), 0x0040_3000);
    bytes[DATA_RAW + 0x280..DATA_RAW + 0x400].fill(0xa5);
    descriptor(&mut bytes, 0, ILT);
    descriptor(&mut bytes, 12, MODULE);
    descriptor(&mut bytes, 16, IAT);
    for (index, &name) in names.iter().enumerate() {
        thunk(&mut bytes, index, name);
    }
    bytes[file(MODULE)..file(MODULE) + 13].copy_from_slice(b"KeRnEl32.dLl\0");
    put16(&mut bytes, file(GET_NAME), 0x1234);
    bytes[file(GET_NAME) + 2..file(GET_NAME) + 15].copy_from_slice(b"GetLastError\0");
    put16(&mut bytes, file(SET_NAME), u16::MAX);
    bytes[file(SET_NAME) + 2..file(SET_NAME) + 15].copy_from_slice(b"SetLastError\0");
    bytes
}

fn relocation(bytes: &mut [u8], target: u32) {
    directory(bytes, 5, RELOCATION, 12);
    put32(bytes, file(RELOCATION), target & !0xfff);
    put32(bytes, file(RELOCATION) + 4, 12);
    put16(
        bytes,
        file(RELOCATION) + 8,
        0x3000 | (target & 0xfff) as u16,
    );
    put16(bytes, file(RELOCATION) + 10, 0);
}

fn bytes_at(memory: &AddressSpace, address: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    memory.read(GuestAddress(address), &mut bytes).unwrap();
    bytes
}

fn word(memory: &AddressSpace, address: u32) -> u32 {
    u32::from_le_bytes(bytes_at(memory, address, 4).try_into().unwrap())
}

fn fault(address: u32, access: Access, reason: FaultReason) -> MemoryError {
    MemoryError::Fault(MemoryFault {
        address: GuestAddress(address),
        access,
        reason,
    })
}

fn refused(bytes: &[u8], expected: LoadError) {
    assert_eq!(
        loader::load_pe32_linked_at(bytes, BASE, GATE, 8).err(),
        Some(expected)
    );
}

fn expected_metadata(
    base: u32,
    gate: u32,
    get: bool,
    set: bool,
    pages: u32,
) -> LinkedImageMetadata32 {
    let empty = GateSpec {
        entry: GuestAddress(0),
        id: 0,
    };
    let mut gates = [empty; 2];
    let mut count = 0;
    for (used, offset, id) in [(get, 0, GET_ID), (set, 16, SET_ID)] {
        if used {
            gates[count] = GateSpec {
                entry: GuestAddress(gate + offset),
                id,
            };
            count += 1;
        }
    }
    LinkedImageMetadata32 {
        image: ImageMetadata32 {
            image_base: base,
            image_size: IMAGE_SIZE,
            entry_point: base + 0x1000,
            mapped_pages: pages,
        },
        gate_base: gate,
        gate_count: count as u32,
        gates,
    }
}

#[test]
fn literal_two_name_image_links_readonly_iat_and_preserves_all_other_image_bytes() {
    let bytes = image(&[SET_NAME, GET_NAME]);
    let original = bytes.clone();
    let (mut memory, metadata) = loader::load_pe32_linked_at(&bytes, BASE, GATE, 5)
        .unwrap()
        .into_parts();
    assert_eq!(metadata, expected_metadata(BASE, GATE, true, true, 5));
    assert_eq!(memory.mapped_pages(), 5);
    assert_eq!(bytes, original);
    assert_eq!(bytes_at(&memory, BASE, 512), bytes[..512]);
    let mut text = vec![0; 4096];
    text[..0x200].copy_from_slice(&bytes[TEXT_RAW..TEXT_RAW + 0x200]);
    assert_eq!(bytes_at(&memory, BASE + 0x1000, 4096), text);
    let mut data = vec![0; 4096];
    data[..0x400].copy_from_slice(&bytes[DATA_RAW..DATA_RAW + 0x400]);
    put32(&mut data, 0x80, GATE + 16);
    put32(&mut data, 0x84, GATE);
    assert_eq!(bytes_at(&memory, BASE + 0x3000, 4096), data);
    assert_eq!(bytes_at(&memory, BASE + 0x5000, 4096), vec![0; 4096]);
    let mut stubs = vec![0; 4096];
    stubs[..2].copy_from_slice(&[0x0f, 0x0b]);
    stubs[16..18].copy_from_slice(&[0x0f, 0x0b]);
    assert_eq!(bytes_at(&memory, GATE, 4096), stubs);
    for address in [BASE, BASE + IAT, GATE, GATE + 4095] {
        let before = bytes_at(&memory, address, 1);
        assert_eq!(
            memory.write(GuestAddress(address), &[0]),
            Err(fault(address, Access::Write, FaultReason::Permission))
        );
        assert_eq!(bytes_at(&memory, address, 1), before);
    }
    for address in [BASE, BASE + IAT, BASE + 0x5000] {
        assert_eq!(
            memory.fetch(GuestAddress(address), &mut [0; 1]),
            Err(fault(address, Access::Execute, FaultReason::Permission))
        );
    }
    memory.fetch(GuestAddress(GATE), &mut [0; 2]).unwrap();
    memory
        .fetch(GuestAddress(BASE + 0x1000), &mut [0; 6])
        .unwrap();
    memory.write(GuestAddress(BASE + 0x5000), &[0x37]).unwrap();
    for rva in [0x2000, 0x4000, 0x6000] {
        assert_eq!(
            memory.read(GuestAddress(BASE + rva), &mut [0; 1]),
            Err(fault(BASE + rva, Access::Read, FaultReason::Unmapped))
        );
    }
}

#[test]
fn duplicate_imports_dedupe_and_pack_only_used_gates_with_optional_iat_directory() {
    for names in [
        &[GET_NAME][..],
        &[SET_NAME][..],
        &[GET_NAME; 8][..],
        &[SET_NAME, GET_NAME, SET_NAME][..],
    ] {
        for with_directory in [false, true] {
            let mut bytes = image(names);
            if !with_directory {
                directory(&mut bytes, 12, 0, 0);
            }
            descriptor(&mut bytes, 8, u32::MAX);
            put16(&mut bytes, file(GET_NAME), u16::MAX);
            let (memory, metadata) = loader::load_pe32_linked_at(&bytes, BASE, GATE, 5)
                .unwrap()
                .into_parts();
            let get = names.contains(&GET_NAME);
            let set = names.contains(&SET_NAME);
            assert_eq!(metadata, expected_metadata(BASE, GATE, get, set, 5));
            for (index, &name) in names.iter().enumerate() {
                assert_eq!(
                    word(&memory, BASE + IAT + index as u32 * 4),
                    GATE + if name == GET_NAME { 0 } else { 16 }
                );
                assert_eq!(word(&memory, BASE + ILT + index as u32 * 4), name);
            }
            assert_eq!(word(&memory, BASE + IAT + names.len() as u32 * 4), 0);
            let mut expected = vec![0; 4096];
            for (used, offset) in [(get, 0), (set, 16)] {
                if used {
                    expected[offset..offset + 2].copy_from_slice(&[0x0f, 0x0b]);
                }
            }
            assert_eq!(bytes_at(&memory, GATE, 4096), expected);
        }
    }
}

#[test]
fn selected_bases_apply_highlow_then_imports_without_changing_source_metadata() {
    let mut bytes = image(&[GET_NAME, SET_NAME]);
    relocation(&mut bytes, 0x1021);
    for (actual, expected) in [
        (BASE, 0x0040_3000),
        (0x0050_0000, 0x0050_3000),
        (0x0030_0000, 0x0030_3000),
    ] {
        let (memory, metadata) = loader::load_pe32_linked_at(&bytes, actual, GATE, 5)
            .unwrap()
            .into_parts();
        assert_eq!(metadata, expected_metadata(actual, GATE, true, true, 5));
        assert_eq!(word(&memory, actual + 0x1021), expected);
        assert_eq!(word(&memory, actual + IAT), GATE);
        assert_eq!(word(&memory, actual + IAT + 4), GATE + 16);
        assert_eq!(word(&memory, actual + OPTIONAL as u32 + 28), BASE);
        for (rva, length) in [
            (DIRECTORY, 40),
            (ILT, 12),
            (MODULE, 13),
            (GET_NAME, 15),
            (SET_NAME, 15),
            (RELOCATION, 12),
        ] {
            assert_eq!(
                bytes_at(&memory, actual + rva, length),
                bytes[file(rva)..file(rva) + length]
            );
        }
    }
    assert_eq!(
        u32::from_le_bytes(bytes[file(0x1021)..file(0x1021) + 4].try_into().unwrap()),
        0x0040_3000
    );
}

#[test]
fn initialized_iat_can_cross_pages_but_must_keep_final_readonly_permissions() {
    let mut bytes = image(&[GET_NAME, SET_NAME]);
    bytes.resize(DATA_RAW + 0x1200, 0);
    put32(&mut bytes, SECTIONS + 40 + 8, 0x1200);
    put32(&mut bytes, SECTIONS + 40 + 16, 0x1200);
    descriptor(&mut bytes, 16, 0x3ffc);
    directory(&mut bytes, 12, 0x3ffc, 12);
    for (index, value) in [GET_NAME, SET_NAME, 0].into_iter().enumerate() {
        put32(&mut bytes, file(0x3ffc) + index * 4, value);
    }
    let (mut memory, metadata) = loader::load_pe32_linked_at(&bytes, BASE, GATE, 6)
        .unwrap()
        .into_parts();
    assert_eq!(metadata.image.mapped_pages, 6);
    assert_eq!(word(&memory, BASE + 0x3ffc), GATE);
    assert_eq!(word(&memory, BASE + 0x4000), GATE + 16);
    assert_eq!(word(&memory, BASE + 0x4004), 0);
    assert_eq!(
        memory.write(GuestAddress(BASE + 0x3ffc), &[0; 8]),
        Err(fault(BASE + 0x3ffc, Access::Write, FaultReason::Permission))
    );
    assert_eq!(word(&memory, BASE + 0x4000), GATE + 16);
}

#[test]
fn directory_descriptor_and_optional_iat_fields_have_frozen_error_classes() {
    for (rva, size, error) in [
        (0, 0, LoadError::Unsupported),
        (0, 40, LoadError::Malformed),
        (DIRECTORY, 0, LoadError::Malformed),
        (DIRECTORY, 20, LoadError::Malformed),
        (DIRECTORY, 39, LoadError::Malformed),
        (DIRECTORY, 41, LoadError::Malformed),
        (DIRECTORY, 60, LoadError::Unsupported),
        (DIRECTORY + 1, 40, LoadError::Malformed),
    ] {
        let mut bytes = image(&[GET_NAME, SET_NAME]);
        directory(&mut bytes, 1, rva, size);
        refused(&bytes, error);
    }
    for field in [0, 4, 8, 12, 16] {
        let mut bytes = image(&[GET_NAME]);
        put32(&mut bytes, file(DIRECTORY) + 20 + field, 1);
        refused(&bytes, LoadError::Malformed);
    }
    for (field, value, error) in [
        (0, 0, LoadError::Unsupported),
        (4, 1, LoadError::Unsupported),
        (8, 1, LoadError::Unsupported),
        (12, 0, LoadError::Malformed),
        (16, 0, LoadError::Malformed),
    ] {
        let mut bytes = image(&[GET_NAME]);
        descriptor(&mut bytes, field, value);
        refused(&bytes, error);
    }
    for (rva, size) in [(0, 8), (IAT, 0), (IAT + 4, 8), (IAT, 4), (IAT, 12)] {
        let mut bytes = image(&[GET_NAME]);
        directory(&mut bytes, 12, rva, size);
        refused(&bytes, LoadError::Malformed);
    }
    for index in (0..16).filter(|index| ![1, 5, 12].contains(index)) {
        let mut bytes = image(&[GET_NAME]);
        directory(&mut bytes, index, 0x3300, 4);
        refused(&bytes, LoadError::Unsupported);
    }
}

#[test]
fn thunk_shapes_and_explicit_ninth_slot_priority_are_bounded() {
    refused(&image(&[]), LoadError::Unsupported);
    for (field, value) in [(0, ILT + 1), (16, IAT + 1), (16, ILT)] {
        let mut bytes = image(&[GET_NAME]);
        descriptor(&mut bytes, field, value);
        refused(&bytes, LoadError::Malformed);
    }
    for (slot, value) in [(0, SET_NAME), (1, GET_NAME)] {
        let mut bytes = image(&[GET_NAME]);
        put32(&mut bytes, file(IAT) + slot * 4, value);
        refused(&bytes, LoadError::Malformed);
    }
    for value in [GET_NAME + 1, 0x7fff_fffe] {
        let mut bytes = image(&[GET_NAME]);
        thunk(&mut bytes, 0, value);
        refused(&bytes, LoadError::Malformed);
    }
    for value in [0x8000_0001, 0xffff_fffe, u32::MAX] {
        let mut bytes = image(&[GET_NAME]);
        thunk(&mut bytes, 0, value);
        refused(&bytes, LoadError::Unsupported);
    }
    for ninth in [GET_NAME, 0x7fff_fffe, u32::MAX] {
        let mut bytes = image(&[GET_NAME; 9]);
        put32(&mut bytes, file(ILT) + 32, ninth);
        put32(&mut bytes, file(IAT) + 32, 0);
        refused(&bytes, LoadError::Capacity);
    }
}

#[test]
fn metadata_provenance_refuses_headers_gaps_bss_padding_and_high_rvas() {
    for rva in [
        0x40,
        0x2000,
        0x3280,
        0x4000,
        0x5000,
        0x7000,
        0xffff_fffc,
        u32::MAX,
    ] {
        for field in [0, 12, 16] {
            let mut bytes = image(&[GET_NAME]);
            descriptor(&mut bytes, field, rva);
            refused(&bytes, LoadError::Malformed);
        }
    }
    for rva in [0x40, 0x2000, 0x3280, 0x4000, 0x5000, 0x7000, 0x7fff_fffe] {
        let mut bytes = image(&[GET_NAME]);
        thunk(&mut bytes, 0, rva);
        refused(&bytes, LoadError::Malformed);
    }
    for rva in [0x40, 0x2000, 0x327c, 0x3280, 0x5000, 0xffff_fffc] {
        let mut bytes = image(&[GET_NAME]);
        directory(&mut bytes, 1, rva, 40);
        refused(&bytes, LoadError::Malformed);
    }
    let mut virtual_tail = image(&[GET_NAME]);
    put32(&mut virtual_tail, SECTIONS + 40 + 8, 0x600);
    descriptor(&mut virtual_tail, 12, 0x3400);
    refused(&virtual_tail, LoadError::Malformed);
    for field in [0, 16] {
        let mut at_last_word = image(&[GET_NAME]);
        descriptor(&mut at_last_word, field, 0x327c);
        put32(&mut at_last_word, file(0x327c), GET_NAME);
        refused(&at_last_word, LoadError::Malformed);
    }
}

#[test]
fn string_character_and_exact_sixty_four_byte_section_boundary_priorities() {
    for symbol in [false, true] {
        for last in [0, b'A', 0x7f] {
            let mut bytes = image(&[GET_NAME]);
            let name = 0x3240;
            if symbol {
                thunk(&mut bytes, 0, name - 2);
                put16(&mut bytes, file(name - 2), 0xffff);
            } else {
                descriptor(&mut bytes, 12, name);
            }
            bytes[file(name)..file(name) + 64].fill(b'A');
            bytes[file(name) + 63] = last;
            refused(
                &bytes,
                match last {
                    0 => LoadError::Unsupported,
                    b'A' => LoadError::Capacity,
                    _ => LoadError::Malformed,
                },
            );
        }
        let mut short = image(&[GET_NAME]);
        let name = 0x3278;
        if symbol {
            thunk(&mut short, 0, name - 2);
        } else {
            descriptor(&mut short, 12, name);
        }
        short[file(name)..file(name) + 8].fill(b'A');
        refused(&short, LoadError::Malformed);
        for value in [0, 0x1f, 0x7f, 0x80, 0xff] {
            let mut bytes = image(&[GET_NAME]);
            bytes[file(if symbol { GET_NAME + 2 } else { MODULE })] = value;
            refused(&bytes, LoadError::Malformed);
        }
    }
    for module in [b"kernel32.exe\0".as_slice(), b"unknown.dll\0".as_slice()] {
        let mut bytes = image(&[GET_NAME]);
        bytes[file(MODULE)..file(MODULE) + 13].fill(0);
        bytes[file(MODULE)..file(MODULE) + module.len()].copy_from_slice(module);
        refused(&bytes, LoadError::Unsupported);
    }
    let mut wrong_case = image(&[GET_NAME]);
    wrong_case[file(GET_NAME) + 2] = b'g';
    refused(&wrong_case, LoadError::Unsupported);
}

fn adjacent_image(names: &[u32]) -> Vec<u8> {
    let mut bytes = image(names);
    bytes.resize(0x1600, 0);
    put32(&mut bytes, SECTIONS + 40 + 8, 0x1000);
    put32(&mut bytes, SECTIONS + 40 + 16, 0x1000);
    section(&mut bytes, 2, 0x4000, 0x200, 0x1400, 0x200, 0x4000_0040);
    bytes
}

#[test]
fn adjacent_initialized_sections_cannot_extend_one_string_record_or_thunk_array() {
    for symbol in [false, true] {
        let mut bytes = adjacent_image(&[GET_NAME]);
        if symbol {
            thunk(&mut bytes, 0, 0x3ffe);
            bytes[file(0x4000)..file(0x4000) + 64].fill(b'A');
        } else {
            descriptor(&mut bytes, 12, 0x3ff0);
            bytes[file(0x3ff0)..file(0x3ff0) + 64].fill(b'A');
        }
        refused(&bytes, LoadError::Malformed);
    }
    for lookup in [false, true] {
        let mut bytes = adjacent_image(&[GET_NAME; 8]);
        descriptor(&mut bytes, if lookup { 0 } else { 16 }, 0x3fe0);
        if !lookup {
            directory(&mut bytes, 12, 0x3fe0, 36);
        }
        for index in 0..8 {
            put32(&mut bytes, file(0x3fe0) + index * 4, GET_NAME);
        }
        put32(&mut bytes, file(0x4000), if lookup { GET_NAME } else { 0 });
        refused(&bytes, LoadError::Malformed);
    }
}

#[test]
fn all_metadata_aliases_fail_before_unknown_name_resolution() {
    let mut module_in_name = image(&[GET_NAME]);
    descriptor(&mut module_in_name, 12, GET_NAME + 2);
    refused(&module_in_name, LoadError::Malformed);
    let mut partial_names = image(&[GET_NAME, GET_NAME + 2]);
    partial_names[file(GET_NAME) + 2..file(GET_NAME) + 15].copy_from_slice(b"Unrecognized\0");
    refused(&partial_names, LoadError::Malformed);
    let mut names_in_lookup = image(&[GET_NAME]);
    descriptor(&mut names_in_lookup, 0, GET_NAME);
    refused(&names_in_lookup, LoadError::Malformed);
    let mut overlapping_arrays = image(&[GET_NAME; 3]);
    descriptor(&mut overlapping_arrays, 16, ILT + 4);
    directory(&mut overlapping_arrays, 12, ILT + 4, 16);
    refused(&overlapping_arrays, LoadError::Malformed);
    // a valid final ten-byte abs block shares only the hint prefix; both structures parse alone.
    let mut hint_prefix = image(&[GET_NAME]);
    directory(&mut hint_prefix, 5, GET_NAME - 8, 10);
    put32(&mut hint_prefix, file(GET_NAME - 8), 0x1000);
    put32(&mut hint_prefix, file(GET_NAME - 8) + 4, 10);
    put16(&mut hint_prefix, file(GET_NAME), 0);
    refused(&hint_prefix, LoadError::Malformed);
}

#[test]
fn highlow_cannot_touch_any_consumed_import_byte_but_alignment_padding_is_not_metadata() {
    for target in [
        DIRECTORY,
        DIRECTORY + 20,
        ILT,
        ILT + 8,
        IAT,
        IAT + 8,
        MODULE,
        MODULE + 12,
        GET_NAME,
        GET_NAME + 14,
        SET_NAME + 14,
    ] {
        let mut bytes = image(&[GET_NAME, SET_NAME]);
        relocation(&mut bytes, target);
        refused(&bytes, LoadError::Malformed);
    }
    let mut padding = image(&[GET_NAME]);
    padding[file(GET_NAME) + 15] = 0xab;
    relocation(&mut padding, GET_NAME + 15);
    let (memory, _) = loader::load_pe32_linked_at(&padding, 0x0050_0000, GATE, 5)
        .unwrap()
        .into_parts();
    assert_eq!(word(&memory, 0x0050_0000 + GET_NAME + 15), 0x540f_ffab);
    assert_eq!(
        bytes_at(&memory, 0x0050_0000 + GET_NAME, 15),
        padding[file(GET_NAME)..file(GET_NAME) + 15]
    );
    let mut stripped = image(&[GET_NAME]);
    relocation(&mut stripped, 0x1021);
    put16(&mut stripped, COFF + 18, 0x0103);
    refused(&stripped, LoadError::Malformed);
    assert_eq!(
        loader::load_pe32_linked_at(&image(&[GET_NAME]), 0x0050_0000, GATE, 5).err(),
        Some(LoadError::Unsupported)
    );
}

#[test]
fn explicit_gate_page_and_total_capacity_include_last_guest_page_and_whole_image_gaps() {
    let bytes = image(&[SET_NAME]);
    for gate in [0, GATE + 1, u32::MAX, BASE, BASE + 0x2000, BASE + 0x6000] {
        assert_eq!(
            loader::load_pe32_linked_at(&bytes, BASE, gate, 8).err(),
            Some(LoadError::Malformed)
        );
    }
    for pages in [0, 4, 4097, u32::MAX] {
        assert_eq!(
            loader::load_pe32_linked_at(&bytes, BASE, GATE, pages).err(),
            Some(LoadError::Capacity)
        );
    }
    let (mut memory, metadata) = loader::load_pe32_linked_at(&bytes, BASE, 0xffff_f000, 5)
        .unwrap()
        .into_parts();
    assert_eq!(
        metadata,
        expected_metadata(BASE, 0xffff_f000, false, true, 5)
    );
    assert_eq!(bytes_at(&memory, 0xffff_f000, 16), vec![0; 16]);
    assert_eq!(bytes_at(&memory, 0xffff_f010, 2), [0x0f, 0x0b]);
    assert_eq!(bytes_at(&memory, u32::MAX, 1), [0]);
    assert_eq!(
        memory.write(GuestAddress(u32::MAX), &[1]),
        Err(fault(u32::MAX, Access::Write, FaultReason::Permission))
    );
    let mut top_image = image(&[GET_NAME]);
    relocation(&mut top_image, 0x1021);
    put32(&mut top_image, OPTIONAL + 56, 0x10000);
    let (memory, metadata) = loader::load_pe32_linked_at(&top_image, 0xffff_0000, GATE, 5)
        .unwrap()
        .into_parts();
    assert_eq!(metadata.image.image_size, 0x10000);
    assert_eq!(metadata.image.entry_point, 0xffff_1000);
    assert_eq!(word(&memory, 0xffff_1021), 0xffff_3000);
    assert_eq!(
        loader::load_pe32_linked_at(&top_image, 0xffff_0000, 0xffff_f000, 5).err(),
        Some(LoadError::Malformed)
    );
    put32(&mut top_image, OPTIONAL + 56, 0x20000);
    assert_eq!(
        loader::load_pe32_linked_at(&top_image, 0xffff_0000, GATE, 5).err(),
        Some(LoadError::Malformed)
    );
    for actual in [0, BASE + 1, BASE + 4096] {
        assert_eq!(
            loader::load_pe32_linked_at(&bytes, actual, GATE, 5).err(),
            Some(LoadError::Malformed)
        );
    }
    let mut oversized_image = image(&[GET_NAME]);
    put32(&mut oversized_image, OPTIONAL + 56, 0x0100_1000);
    refused(&oversized_image, LoadError::Capacity);
    let mut oversized_file = image(&[GET_NAME]);
    oversized_file.resize(16 * 1024 * 1024 + 1, 0);
    refused(&oversized_file, LoadError::Capacity);
}

type ModuleView = Result<(Vec<u8>, usize), HostError>;

#[derive(Debug, PartialEq, Eq)]
struct ProcessState {
    arena: Vec<u8>,
    arena_pointer: usize,
    key: u64,
    generation: u32,
    dispatcher: ModuleView,
    artifact: ModuleView,
    unit: Option<ModuleView>,
    pages: Result<(u32, u32), HostError>,
    ram: Vec<Result<Vec<u8>, HostError>>,
}

fn process_state(engine: &EngineInstance, unit: Option<u64>) -> ProcessState {
    ProcessState {
        arena: engine.arena().to_vec(),
        arena_pointer: engine.arena_address(),
        key: engine.key(),
        generation: engine.generation(),
        dispatcher: engine
            .dispatcher_bytes(engine.key())
            .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize)),
        artifact: engine
            .artifact_bytes()
            .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize)),
        unit: unit.map(|id| {
            engine
                .resident_bytes(id)
                .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize))
        }),
        pages: engine
            .memory()
            .map(|memory| (memory.capacity_pages(), memory.mapped_pages())),
        ram: [
            0,
            0x1000,
            BASE,
            BASE + 0x1000,
            BASE + 0x3000,
            BASE + 0x5000,
            GATE,
        ]
        .into_iter()
        .map(|address| {
            let mut bytes = vec![0; 4096];
            engine
                .memory()?
                .read(GuestAddress(address), &mut bytes)
                .map_err(HostError::Memory)?;
            Ok(bytes)
        })
        .collect(),
    }
}

fn process_refused<T>(
    engine: &mut EngineInstance,
    unit: Option<u64>,
    expected: HostError,
    operation: impl FnOnce(&mut EngineInstance) -> Result<T, HostError>,
) {
    let before = process_state(engine, unit);
    assert_eq!(operation(engine).err(), Some(expected));
    assert_eq!(process_state(engine, unit), before);
}

#[test]
fn old_fixed_and_selected_base_apis_keep_strict_import_refusal_and_can_retry_linked() {
    let bytes = image(&[GET_NAME, SET_NAME]);
    assert_eq!(
        loader::load_pe32(&bytes, 8).err(),
        Some(LoadError::Unsupported)
    );
    assert_eq!(
        loader::load_pe32_at(&bytes, BASE, 8).err(),
        Some(LoadError::Unsupported)
    );
    let mut engine = EngineInstance::new(8, KEY).unwrap();
    engine.arena_mut().unwrap().fill(0x6d);
    process_refused(
        &mut engine,
        None,
        HostError::Loader(LoadError::Unsupported),
        |engine| engine.load_pe32(&bytes),
    );
    process_refused(
        &mut engine,
        None,
        HostError::Loader(LoadError::Unsupported),
        |engine| engine.load_pe32_at(&bytes, BASE),
    );
    let arena = engine.arena().to_vec();
    assert_eq!(
        engine.load_pe32_linked_at(&bytes, BASE, GATE),
        Ok(expected_metadata(BASE, GATE, true, true, 5))
    );
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.key(), KEY);
    assert_eq!(engine.generation(), 0);
}

#[test]
fn failed_pristine_link_preserves_public_process_identity_and_does_not_spend_image_latch() {
    let valid = image(&[GET_NAME, SET_NAME]);
    let mut malformed = valid.clone();
    put32(&mut malformed, file(IAT), 0);
    let mut unsupported = valid.clone();
    unsupported[file(GET_NAME) + 2] = b'g';
    let capacity = image(&[GET_NAME; 9]);
    for (bytes, error) in [
        (malformed, LoadError::Malformed),
        (unsupported, LoadError::Unsupported),
        (capacity, LoadError::Capacity),
    ] {
        let mut engine = EngineInstance::new(6, KEY).unwrap();
        engine.arena_mut().unwrap().fill(0x9e);
        process_refused(&mut engine, None, HostError::Loader(error), |engine| {
            engine.load_pe32_linked_at(&bytes, BASE, GATE)
        });
        let arena = engine.arena().to_vec();
        let dispatcher = engine.dispatcher_bytes(KEY).unwrap().as_ptr();
        assert_eq!(
            engine.load_pe32_linked_at(&valid, BASE, GATE),
            Ok(expected_metadata(BASE, GATE, true, true, 5))
        );
        assert_eq!(engine.arena(), arena);
        assert_eq!(engine.dispatcher_bytes(KEY).unwrap().as_ptr(), dispatcher);
        assert_eq!(engine.memory().unwrap().capacity_pages(), 6);
    }
    let mut insufficient = EngineInstance::new(4, KEY).unwrap();
    process_refused(
        &mut insufficient,
        None,
        HostError::Loader(LoadError::Capacity),
        |engine| engine.load_pe32_linked_at(&valid, BASE, GATE),
    );
    insufficient.map(0x1000, 1, 3).unwrap();
    assert_eq!(
        bytes_at(insufficient.memory().unwrap(), 0x1000, 4096),
        vec![0; 4096]
    );
}

#[test]
fn existing_ram_and_retained_code_owners_precede_all_linker_input_errors() {
    let mut engine = EngineInstance::new(8, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140] = 0x90;
    engine.upload(0x1000, 1).unwrap();
    let snapshot = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(0x1000), 1)
        .unwrap();
    process_refused(&mut engine, None, HostError::InvalidRequest, |engine| {
        engine.load_pe32_linked_at(b"bad", 0, 0)
    });
    assert!(engine.memory().unwrap().is_code_current(&snapshot));
    engine.arena_mut().unwrap()[140..148].copy_from_slice(&[0, 0x10, 0, 0, 1, 0, 0, 0]);
    let unit = engine.compile_resident_with_gates(1, 0).unwrap().get();
    engine.compile(1).unwrap();
    process_refused(
        &mut engine,
        Some(unit),
        HostError::InvalidRequest,
        |engine| engine.load_pe32_linked_at(&image(&[GET_NAME]), BASE, GATE),
    );
    assert!(engine.memory().unwrap().is_code_current(&snapshot));
    engine.unmap(0x1000, 1).unwrap();
    assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
    process_refused(
        &mut engine,
        Some(unit),
        HostError::InvalidRequest,
        |engine| engine.load_pe32_linked_at(b"bad", 0, 0),
    );
}

#[test]
fn successful_image_latch_survives_all_unmaps_and_closed_wins_every_error() {
    let bytes = image(&[GET_NAME]);
    let mut engine = EngineInstance::new(5, KEY).unwrap();
    engine.load_pe32_linked_at(&bytes, BASE, GATE).unwrap();
    for address in [BASE, BASE + 0x1000, BASE + 0x3000, BASE + 0x5000, GATE] {
        engine.unmap(address, 1).unwrap();
    }
    assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
    assert_eq!(engine.generation(), 0);
    process_refused(&mut engine, None, HostError::InvalidRequest, |engine| {
        engine.load_pe32_linked_at(b"bad", 0, 0)
    });
    process_refused(&mut engine, None, HostError::InvalidRequest, |engine| {
        engine.load_pe32(&bytes)
    });
    let arena = engine.arena().to_vec();
    engine.close();
    assert_eq!(engine.arena(), arena);
    for (input, actual, gate) in [
        (bytes.as_slice(), BASE, GATE),
        (b"bad".as_slice(), 0, 0),
        (&[][..], u32::MAX, u32::MAX),
    ] {
        process_refused(&mut engine, None, HostError::Closed, |engine| {
            engine.load_pe32_linked_at(input, actual, gate)
        });
    }
    assert!(!engine.is_open());
    assert_eq!(engine.arena_mut(), Err(HostError::Closed));
}
