#[allow(dead_code)]
#[path = "support/pe32.rs"]
mod pe;

use ring3_engine::{
    abi::{
        arena::TRANSFER_OFFSET,
        x86::{decode_exit, encode_exit_v3, encode_state},
    },
    cpu::{ExecutionExit, ExitReason, dbt::GateSpec, x86::State32},
    loader::{self, ImageMetadata32, LinkedImageMetadata32V2, LoadError},
    memory::{Access, AddressSpace, FaultReason, GuestAddress, MemoryError, MemoryFault},
    process::{EngineInstance, HostError},
    windows::CallingConvention32,
};

const KEY: u64 = 0xe345_0000_0000_0001;
const GATE: u32 = 0x7000_0000;
const ILT: u32 = 0x3040;
const IAT: u32 = 0x3080;
const MODULE: u32 = 0x30c0;
const NAMES: [u32; 3] = [0x3100, 0x3120, 0x3140];
const IDS: [u32; 3] = [0x0001_0001, 0x0001_0002, 0x0001_0003];

fn file(rva: u32) -> usize {
    if rva < 0x3000 {
        pe::TEXT_RAW + (rva - 0x1000) as usize
    } else {
        pe::DATA_RAW + (rva - 0x3000) as usize
    }
}

fn directory(bytes: &mut [u8], index: usize, rva: u32, size: u32) {
    pe::put32(bytes, pe::OPTIONAL + 96 + index * 8, rva);
    pe::put32(bytes, pe::OPTIONAL + 100 + index * 8, size);
}

fn image(names: &[u32]) -> Vec<u8> {
    // literal sources reuse only the independent basic pe header builder.
    let mut bytes = pe::image();
    pe::put16(&mut bytes, pe::COFF + 18, 0x0102);
    pe::put32(&mut bytes, pe::section(0) + 8, 0x200);
    pe::put32(&mut bytes, pe::section(1) + 36, 0x4000_0040);
    bytes[pe::TEXT_RAW..pe::TEXT_RAW + 512].fill(0x90);
    bytes[pe::TEXT_RAW..pe::TEXT_RAW + 5].copy_from_slice(&[0xb8, 42, 0, 0, 0]);
    pe::put32(&mut bytes, pe::TEXT_RAW + 0x21, pe::BASE + 0x5000);
    bytes[pe::DATA_RAW..].fill(0);
    directory(&mut bytes, 1, 0x3000, 40);
    directory(&mut bytes, 12, IAT, 4 * (names.len() as u32 + 1));
    for (field, value) in [(0, ILT), (12, MODULE), (16, IAT)] {
        pe::put32(&mut bytes, pe::DATA_RAW + field, value);
    }
    for (index, &name) in names.iter().enumerate() {
        pe::put32(&mut bytes, file(ILT) + index * 4, name);
        pe::put32(&mut bytes, file(IAT) + index * 4, name);
    }
    bytes[file(MODULE)..file(MODULE) + 13].copy_from_slice(b"KeRnEl32.dLl\0");
    for (index, symbol) in [
        b"GetLastError\0".as_slice(),
        b"SetLastError\0",
        b"ExitProcess\0",
    ]
    .into_iter()
    .enumerate()
    {
        pe::put16(&mut bytes, file(NAMES[index]), 0x1234 + index as u16);
        let at = file(NAMES[index]) + 2;
        bytes[at..at + symbol.len()].copy_from_slice(symbol);
    }
    bytes
}

fn relocate(bytes: &mut [u8], target: u32) {
    directory(bytes, 5, 0x31c0, 12);
    pe::put32(bytes, file(0x31c0), target & !0xfff);
    pe::put32(bytes, file(0x31c0) + 4, 12);
    pe::put16(bytes, file(0x31c0) + 8, 0x3000 | (target & 0xfff) as u16);
    pe::put16(bytes, file(0x31c0) + 10, 0);
}

fn read(memory: &AddressSpace, address: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    memory.read(GuestAddress(address), &mut bytes).unwrap();
    bytes
}

fn word(memory: &AddressSpace, address: u32) -> u32 {
    u32::from_le_bytes(read(memory, address, 4).try_into().unwrap())
}

fn expected(base: u32, gate: u32, used: [bool; 3]) -> LinkedImageMetadata32V2 {
    let mut gates = [GateSpec {
        entry: GuestAddress(0),
        id: 0,
    }; 3];
    let mut count = 0;
    for index in 0..3 {
        if used[index] {
            gates[count] = GateSpec {
                entry: GuestAddress(gate + index as u32 * 16),
                id: IDS[index],
            };
            count += 1;
        }
    }
    LinkedImageMetadata32V2 {
        image: ImageMetadata32 {
            image_base: base,
            image_size: pe::IMAGE_SIZE,
            entry_point: base + 0x1000,
            mapped_pages: 5,
        },
        gate_base: gate,
        gate_count: count as u32,
        gates,
    }
}

fn refused(bytes: &[u8], error: LoadError) {
    assert_eq!(
        loader::load_pe32_linked_v2_at(bytes, pe::BASE, GATE, 5).err(),
        Some(error)
    );
}

fn permission(address: u32, access: Access) -> MemoryError {
    MemoryError::Fault(MemoryFault {
        address: GuestAddress(address),
        access,
        reason: FaultReason::Permission,
    })
}

fn descriptors(engine: &mut EngineInstance, gates: &[GateSpec]) {
    let words: Vec<u32> = gates
        .iter()
        .flat_map(|gate| [gate.entry.0, 2])
        .chain(gates.iter().flat_map(|gate| [gate.entry.0, gate.id]))
        .collect();
    for (index, word) in words.into_iter().enumerate() {
        let at = TRANSFER_OFFSET + index * 4;
        engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&word.to_le_bytes());
    }
}

#[test]
fn all_seven_subsets_pack_used_ids_and_leave_the_complete_unused_gate_page_zero() {
    for mask in 1..8 {
        let used = [mask & 1 != 0, mask & 2 != 0, mask & 4 != 0];
        let names: Vec<u32> = NAMES
            .into_iter()
            .enumerate()
            .filter_map(|(index, name)| used[index].then_some(name))
            .collect();
        let bytes = image(&names);
        let original = bytes.clone();
        let (memory, linked) = loader::load_pe32_linked_v2_at(&bytes, pe::BASE, GATE, 5)
            .unwrap()
            .into_parts();
        assert_eq!(linked, expected(pe::BASE, GATE, used));
        assert_eq!(memory.mapped_pages(), 5);
        let mut stubs = vec![0; 4096];
        for index in 0..3 {
            if used[index] {
                stubs[index * 16..index * 16 + 2].copy_from_slice(&[0x0f, 0x0b]);
            }
        }
        assert_eq!(read(&memory, GATE, 4096), stubs);
        for (index, name) in names.into_iter().enumerate() {
            let provider = NAMES.iter().position(|&value| value == name).unwrap();
            assert_eq!(
                word(&memory, pe::BASE + IAT + index as u32 * 4),
                GATE + provider as u32 * 16
            );
            assert_eq!(word(&memory, pe::BASE + ILT + index as u32 * 4), name);
        }
        assert_eq!(
            word(
                &memory,
                pe::BASE + IAT + used.into_iter().filter(|&value| value).count() as u32 * 4
            ),
            0
        );
        assert_eq!(bytes, original);
    }
}

#[test]
fn mixed_order_duplicate_names_and_unselective_hints_share_stable_stubs() {
    for names in [
        &[NAMES[2], NAMES[0], NAMES[1], NAMES[2]][..],
        &[NAMES[2]; 8][..],
    ] {
        for optional_iat in [false, true] {
            let mut bytes = image(names);
            if !optional_iat {
                directory(&mut bytes, 12, 0, 0);
            }
            for name in NAMES {
                pe::put16(&mut bytes, file(name), u16::MAX);
            }
            let (memory, linked) = loader::load_pe32_linked_v2_at(&bytes, pe::BASE, GATE, 5)
                .unwrap()
                .into_parts();
            assert_eq!(
                linked,
                expected(pe::BASE, GATE, NAMES.map(|name| names.contains(&name)))
            );
            for (index, &name) in names.iter().enumerate() {
                let offset = NAMES.iter().position(|&value| value == name).unwrap() as u32 * 16;
                assert_eq!(
                    word(&memory, pe::BASE + IAT + index as u32 * 4),
                    GATE + offset
                );
            }
        }
    }
}

#[test]
fn full_image_bytes_permissions_zero_tails_and_unmapped_gaps_are_preserved() {
    let bytes = image(&NAMES);
    let (mut memory, linked) = loader::load_pe32_linked_v2_at(&bytes, pe::BASE, GATE, 5)
        .unwrap()
        .into_parts();
    assert_eq!(linked, expected(pe::BASE, GATE, [true; 3]));
    for (rva, source) in [(0, 0), (0x1000, pe::TEXT_RAW), (0x3000, pe::DATA_RAW)] {
        let mut page = vec![0; 4096];
        page[..512].copy_from_slice(&bytes[source..source + 512]);
        if rva == 0x3000 {
            for index in 0..3 {
                pe::put32(&mut page, 0x80 + index * 4, GATE + index as u32 * 16);
            }
        }
        assert_eq!(read(&memory, pe::BASE + rva, 4096), page);
    }
    assert_eq!(read(&memory, pe::BASE + 0x5000, 4096), vec![0; 4096]);
    for address in [pe::BASE, pe::BASE + IAT, GATE, GATE + 32, GATE + 4095] {
        let before = read(&memory, address, 1);
        assert_eq!(
            memory.write(GuestAddress(address), &[0x55]),
            Err(permission(address, Access::Write))
        );
        assert_eq!(read(&memory, address, 1), before);
    }
    for address in [pe::BASE, pe::BASE + IAT, pe::BASE + 0x5000] {
        assert_eq!(
            memory.fetch(GuestAddress(address), &mut [0; 1]),
            Err(permission(address, Access::Execute))
        );
    }
    memory.fetch(GuestAddress(GATE + 32), &mut [0; 2]).unwrap();
    memory
        .write(GuestAddress(pe::BASE + 0x5000), &[0x37])
        .unwrap();
    for rva in [0x2000, 0x4000, 0x6000] {
        assert!(matches!(
            memory.read(GuestAddress(pe::BASE + rva), &mut [0; 1]),
            Err(MemoryError::Fault(MemoryFault {
                reason: FaultReason::Unmapped,
                ..
            }))
        ));
    }
}

#[test]
fn preferred_and_selected_bases_apply_highlow_and_own_bytes_after_source_release() {
    for base in [pe::BASE, 0x0050_0000] {
        let (memory, linked) = {
            let mut bytes = image(&[NAMES[2], NAMES[1], NAMES[0]]);
            relocate(&mut bytes, 0x1021);
            let original = bytes.clone();
            let loaded = loader::load_pe32_linked_v2_at(&bytes, base, GATE, 5).unwrap();
            assert_eq!(bytes, original);
            loaded.into_parts()
        };
        assert_eq!(linked, expected(base, GATE, [true; 3]));
        assert_eq!(word(&memory, base + 0x1021), base + 0x5000);
        assert_eq!(word(&memory, base + IAT), GATE + 32);
        assert_eq!(word(&memory, base + IAT + 4), GATE + 16);
        assert_eq!(word(&memory, base + IAT + 8), GATE);
        assert_eq!(word(&memory, base + pe::OPTIONAL as u32 + 28), pe::BASE);
    }
}

#[test]
fn old_linked_profile_refuses_exit_and_retains_exact_get_set_receipt() {
    for names in [&[NAMES[2]][..], &NAMES[..], &[NAMES[0], NAMES[2]][..]] {
        let bytes = image(names);
        assert_eq!(
            loader::load_pe32_linked_at(&bytes, pe::BASE, GATE, 5).err(),
            Some(LoadError::Unsupported)
        );
        assert_eq!(
            loader::load_pe32(&bytes, 5).err(),
            Some(LoadError::Unsupported)
        );
        assert_eq!(
            loader::load_pe32_at(&bytes, pe::BASE, 5).err(),
            Some(LoadError::Unsupported)
        );
    }
    let (memory, old) =
        loader::load_pe32_linked_at(&image(&[NAMES[1], NAMES[0]]), pe::BASE, GATE, 5)
            .unwrap()
            .into_parts();
    let new = expected(pe::BASE, GATE, [true, true, false]);
    assert_eq!(old.image, new.image);
    assert_eq!(old.gate_base, new.gate_base);
    assert_eq!(old.gate_count, 2);
    assert_eq!(old.gates, [new.gates[0], new.gates[1]]);
    assert_eq!(word(&memory, pe::BASE + IAT), GATE + 16);
    assert_eq!(read(&memory, GATE + 32, 16), vec![0; 16]);
    let mut engine = EngineInstance::new(5, KEY).unwrap();
    let arena = engine.arena().to_vec();
    assert_eq!(
        engine.load_pe32_linked_at(&image(&[NAMES[2]]), pe::BASE, GATE),
        Err(HostError::Loader(LoadError::Unsupported))
    );
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
}

#[test]
fn final_gate_page_and_exact_image_adjacency_are_valid_but_whole_image_gaps_overlap() {
    let bytes = image(&[NAMES[2]]);
    for gate in [0xffff_f000, pe::BASE - 4096, pe::BASE + pe::IMAGE_SIZE] {
        let (mut memory, linked) = loader::load_pe32_linked_v2_at(&bytes, pe::BASE, gate, 5)
            .unwrap()
            .into_parts();
        assert_eq!(linked, expected(pe::BASE, gate, [false, false, true]));
        let mut page = vec![0; 4096];
        page[32..34].copy_from_slice(&[0x0f, 0x0b]);
        assert_eq!(read(&memory, gate, 4096), page);
        if gate == 0xffff_f000 {
            assert_eq!(
                memory.write(GuestAddress(u32::MAX), &[1]),
                Err(permission(u32::MAX, Access::Write))
            );
        }
    }
    for gate in [
        0,
        GATE + 1,
        0xffff_f001,
        pe::BASE,
        pe::BASE + 0x2000,
        pe::BASE + 0x6000,
    ] {
        assert_eq!(
            loader::load_pe32_linked_v2_at(&bytes, pe::BASE, gate, 4).err(),
            Some(LoadError::Malformed)
        );
    }
}

#[test]
fn global_capacity_and_ninth_ilt_priorities_remain_bounded() {
    for pages in [0, 4097, u32::MAX] {
        assert_eq!(
            loader::load_pe32_linked_v2_at(b"bad", 0, 0, pages).err(),
            Some(LoadError::Capacity)
        );
    }
    let bytes = image(&NAMES);
    assert_eq!(
        loader::load_pe32_linked_v2_at(&bytes, pe::BASE, GATE, 4).err(),
        Some(LoadError::Capacity)
    );
    assert_eq!(
        loader::load_pe32_linked_v2_at(&bytes, pe::BASE, GATE, 4096)
            .unwrap()
            .into_parts()
            .1
            .image
            .mapped_pages,
        5
    );
    let mut large = bytes.clone();
    large.resize(16 * 1024 * 1024 + 1, 0);
    assert_eq!(
        loader::load_pe32_linked_v2_at(&large, 0, 0, 5).err(),
        Some(LoadError::Capacity)
    );
    let mut image_cap = bytes;
    pe::put32(&mut image_cap, pe::OPTIONAL + 56, 16 * 1024 * 1024 + 4096);
    refused(&image_cap, LoadError::Capacity);
    for ninth in [NAMES[2], 0x7fff_fffe, u32::MAX] {
        let mut too_many = image(&[NAMES[2]; 9]);
        pe::put32(&mut too_many, file(ILT) + 32, ninth);
        pe::put32(&mut too_many, file(IAT) + 32, 0);
        refused(&too_many, LoadError::Capacity);
    }
}

#[test]
fn new_provider_keeps_initialized_section_source_bounds_and_high_rva_refusal() {
    for rva in [0x40, 0x2000, 0x3200, 0x4000, 0x5000, 0xffff_fffc] {
        for field in [0, 12, 16] {
            let mut bytes = image(&[NAMES[2]]);
            pe::put32(&mut bytes, pe::DATA_RAW + field, rva);
            refused(&bytes, LoadError::Malformed);
        }
    }
    for rva in [0x40, 0x2000, 0x3200, 0x5000, 0x7fff_fffe] {
        let mut bytes = image(&[NAMES[2]]);
        pe::put32(&mut bytes, file(ILT), rva);
        pe::put32(&mut bytes, file(IAT), rva);
        refused(&bytes, LoadError::Malformed);
    }
    let mut padding = image(&[NAMES[2]]);
    pe::put32(&mut padding, pe::section(1) + 8, 0x180);
    pe::put32(&mut padding, pe::DATA_RAW + 12, 0x3180);
    padding[file(0x3180)..file(0x3180) + 13].copy_from_slice(b"kernel32.dll\0");
    refused(&padding, LoadError::Malformed);
}

#[test]
fn structural_aliases_win_before_unknown_symbol_or_exit_resolution() {
    for alias in 0..3 {
        let mut bytes = image(&[NAMES[2]]);
        bytes[file(NAMES[2]) + 2] = b'x';
        match alias {
            0 => {
                pe::put32(&mut bytes, pe::DATA_RAW + 16, ILT);
                directory(&mut bytes, 12, ILT, 8);
            }
            1 => pe::put32(&mut bytes, pe::DATA_RAW + 12, NAMES[2] + 2),
            _ => relocate(&mut bytes, IAT),
        }
        refused(&bytes, LoadError::Malformed);
    }
    let mut stripped = image(&NAMES);
    relocate(&mut stripped, 0x1021);
    pe::put16(&mut stripped, pe::COFF + 18, 0x0103);
    refused(&stripped, LoadError::Malformed);
}

#[test]
fn optional_iat_shape_and_closed_named_resolver_keep_existing_refusals() {
    for (rva, size) in [(0, 16), (IAT, 0), (IAT + 4, 16), (IAT, 12), (IAT, 20)] {
        let mut bytes = image(&NAMES);
        directory(&mut bytes, 12, rva, size);
        refused(&bytes, LoadError::Malformed);
    }
    for mode in 0..4 {
        let mut bytes = image(&[NAMES[2]]);
        match mode {
            0 => pe::put32(&mut bytes, pe::DATA_RAW, 0),
            1 => {
                pe::put32(&mut bytes, file(ILT), 0x8000_0001);
                pe::put32(&mut bytes, file(IAT), 0x8000_0001);
            }
            2 => bytes[file(NAMES[2]) + 2] = b'e',
            _ => bytes[file(MODULE)] = b'x',
        }
        refused(&bytes, LoadError::Unsupported);
    }
}

#[test]
fn pristine_failure_keeps_full_arena_and_valid_load_preserves_nondefault_context() {
    let mut engine = EngineInstance::new(5, KEY).unwrap();
    let address = engine.arena_address();
    for (index, byte) in engine.arena_mut().unwrap().iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(37).wrapping_add(11);
    }
    let before = engine.arena().to_vec();
    let dispatcher = engine.dispatcher_bytes(KEY).unwrap().to_vec();
    for bytes in [b"bad".to_vec(), {
        let mut bytes = image(&NAMES);
        bytes[file(NAMES[2]) + 2] = b'x';
        bytes
    }] {
        let error = if bytes.len() == 3 {
            LoadError::Malformed
        } else {
            LoadError::Unsupported
        };
        assert_eq!(
            engine.load_pe32_linked_v2_at(&bytes, pe::BASE, GATE),
            Err(HostError::Loader(error))
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
        assert_eq!(engine.arena_address(), address);
    }
    assert_eq!(
        engine
            .load_pe32_linked_v2_at(&image(&NAMES), pe::BASE, GATE)
            .unwrap(),
        expected(pe::BASE, GATE, [true; 3])
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.arena_address(), address);
    assert_eq!(engine.dispatcher_bytes(KEY).unwrap(), dispatcher);
    assert_eq!(engine.generation(), 0);
}

#[test]
fn nonpristine_code_and_retained_units_refuse_new_loading_without_mutation() {
    for resident in [false, true] {
        let mut engine = EngineInstance::new(5, KEY).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 5]
            .copy_from_slice(&[0xb8, 42, 0, 0, 0]);
        engine.upload(0x1000, 5).unwrap();
        let snapshot = engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(0x1000), 5)
            .unwrap();
        descriptors(
            &mut engine,
            &[GateSpec {
                entry: GuestAddress(0x1000),
                id: IDS[2],
            }],
        );
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + 4..TRANSFER_OFFSET + 8]
            .copy_from_slice(&5_u32.to_le_bytes());
        let unit = if resident {
            Some(engine.compile_resident(1).unwrap().get())
        } else {
            engine.compile(1).unwrap();
            None
        };
        let code = if let Some(id) = unit {
            engine.resident_bytes(id).unwrap().to_vec()
        } else {
            engine.artifact_bytes().unwrap().to_vec()
        };
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.load_pe32_linked_v2_at(b"bad", 0, 0),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(
            read(engine.memory().unwrap(), 0x1000, 5),
            [0xb8, 42, 0, 0, 0]
        );
        assert!(engine.memory().unwrap().is_code_current(&snapshot));
        let retained = if let Some(id) = unit {
            engine.resident_bytes(id).unwrap()
        } else {
            engine.artifact_bytes().unwrap()
        };
        assert_eq!(retained, code);
        engine.unmap(0x1000, 1).unwrap();
        assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.load_pe32_linked_v2_at(&image(&NAMES), pe::BASE, GATE),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.arena(), before);
    }
}

#[test]
fn loaded_image_latch_survives_unmapping_and_close_has_first_priority() {
    let mut engine = EngineInstance::new(6, KEY).unwrap();
    let linked = engine
        .load_pe32_linked_v2_at(&image(&NAMES), pe::BASE, GATE)
        .unwrap();
    engine.start_loaded_image(0x8000, 1).unwrap();
    for address in [
        pe::BASE,
        pe::BASE + 0x1000,
        pe::BASE + 0x3000,
        pe::BASE + 0x5000,
        GATE,
        0x8000,
    ] {
        engine.unmap(address, 1).unwrap();
    }
    assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.load_pe32_linked_v2_at(b"bad", 0, 0),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(
        engine.start_loaded_image(0x8000, 1),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(linked.image.entry_point, pe::BASE + 0x1000);
    engine.close();
    assert_eq!(
        engine.load_pe32_linked_v2_at(b"bad", 0, 0),
        Err(HostError::Closed)
    );
    assert_eq!(engine.arena(), before);
}

#[test]
fn linked_terminal_provider_uses_saved_argument_and_precedes_new_load_input_validation() {
    // typed stop injection models lifecycle; actual wasm owns guest call execution.
    let mut engine = EngineInstance::new(6, KEY).unwrap();
    let linked = engine
        .load_pe32_linked_v2_at(&image(&NAMES), pe::BASE, GATE)
        .unwrap();
    descriptors(&mut engine, &linked.gates);
    let generation = engine.compile_with_gates(3, 3).unwrap();
    engine.start_loaded_image(0x8000, 1).unwrap();
    engine.write32(0x8ff8, pe::BASE + 0x1005).unwrap();
    engine.write32(0x8ffc, 0xf123_4567).unwrap();
    let mut state = State32::default();
    state.registers[0] = 0x89ab_cdef;
    state.registers[4] = 0x8ff8;
    state.eip = linked.gates[2].entry.0;
    let arena = engine.arena_mut().unwrap();
    encode_state(&state, &mut arena[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 3,
            reason: ExitReason::Gate { id: IDS[2] },
        },
        &mut arena[56..96],
    )
    .unwrap();
    let call = engine
        .capture_call(KEY, generation, CallingConvention32::Stdcall, 1)
        .unwrap();
    engine.write32(0x8ffc, 0x89ab_cdef).unwrap();
    engine
        .complete_windows_call(KEY, generation, call.token)
        .unwrap();
    assert_eq!(
        decode_exit(&engine.arena()[56..96]).unwrap(),
        ExecutionExit {
            retired: 0,
            reason: ExitReason::ProcessExited { code: 0xf123_4567 }
        }
    );
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.load_pe32_linked_v2_at(b"bad", 0, 0),
        Err(HostError::ProcessExited)
    );
    assert_eq!(engine.arena(), before);
    engine.close();
    assert_eq!(
        engine.load_pe32_linked_v2_at(b"bad", 0, 0),
        Err(HostError::Closed)
    );
    assert_eq!(engine.arena(), before);
}
