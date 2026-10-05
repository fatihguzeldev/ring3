#[allow(dead_code)]
#[path = "support/pe32.rs"]
mod pe;

use ring3_engine::{
    cpu::dbt::GateSpec,
    loader::{self, ImageMetadata32, LinkedImageMetadata32V5},
    memory::{AddressSpace, GuestAddress},
    process::EngineInstance,
};

const KEY: u64 = 0xe005_ffff_1234_5678;
const GATE: u32 = 0x7000_0000;
const ILT: u32 = 0x3040;
const IAT: u32 = 0x3080;
const MODULE: u32 = 0x30c0;
const FREE_NAME: u32 = 0x3100;
const ALLOC_NAME: u32 = 0x3120;

fn data_file(rva: u32) -> usize {
    pe::DATA_RAW + (rva - 0x3000) as usize
}

fn directory(bytes: &mut [u8], index: usize, rva: u32, size: u32) {
    pe::put32(bytes, pe::OPTIONAL + 96 + index * 8, rva);
    pe::put32(bytes, pe::OPTIONAL + 100 + index * 8, size);
}

fn named_free_and_alloc_image() -> Vec<u8> {
    // independent basic pe headers; named imports and expectations are literal.
    let mut bytes = pe::image();
    bytes[pe::DATA_RAW..pe::DATA_RAW + 512].fill(0);
    pe::put32(&mut bytes, pe::section(1) + 36, 0x4000_0040);
    directory(&mut bytes, 1, 0x3000, 40);
    directory(&mut bytes, 12, IAT, 12);
    for (field, value) in [(0, ILT), (12, MODULE), (16, IAT)] {
        pe::put32(&mut bytes, pe::DATA_RAW + field, value);
    }
    // source order deliberately differs from canonical metadata slot order.
    for (index, name) in [FREE_NAME, ALLOC_NAME].into_iter().enumerate() {
        pe::put32(&mut bytes, data_file(ILT) + index * 4, name);
        pe::put32(&mut bytes, data_file(IAT) + index * 4, name);
    }
    let module = b"KeRnEl32.dLl\0";
    bytes[data_file(MODULE)..data_file(MODULE) + module.len()].copy_from_slice(module);
    for (name, hint, symbol) in [
        (FREE_NAME, 0x1234, b"VirtualFree\0".as_slice()),
        (ALLOC_NAME, 0x5678, b"VirtualAlloc\0".as_slice()),
    ] {
        pe::put16(&mut bytes, data_file(name), hint);
        let at = data_file(name) + 2;
        bytes[at..at + symbol.len()].copy_from_slice(symbol);
    }
    bytes
}

fn read(memory: &AddressSpace, address: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    memory.read(GuestAddress(address), &mut bytes).unwrap();
    bytes
}

fn expected_metadata() -> LinkedImageMetadata32V5 {
    let empty = GateSpec {
        entry: GuestAddress(0),
        id: 0,
    };
    LinkedImageMetadata32V5 {
        image: ImageMetadata32 {
            image_base: pe::BASE,
            image_size: pe::IMAGE_SIZE,
            entry_point: pe::BASE + 0x1000,
            mapped_pages: 5,
        },
        gate_base: GATE,
        gate_count: 2,
        gates: [
            GateSpec {
                entry: GuestAddress(GATE + 64),
                id: 0x0001_0005,
            },
            GateSpec {
                entry: GuestAddress(GATE + 80),
                id: 0x0001_0006,
            },
            empty,
            empty,
            empty,
            empty,
        ],
    }
}

fn assert_named_link(memory: &AddressSpace, linked: LinkedImageMetadata32V5, input: &[u8]) {
    assert_eq!(linked, expected_metadata());
    assert_eq!(linked.gates.len(), 6);
    assert_eq!(memory.mapped_pages(), 5);
    let mut expected_gates = vec![0; 4096];
    expected_gates[64..66].copy_from_slice(&[0x0f, 0x0b]);
    expected_gates[80..82].copy_from_slice(&[0x0f, 0x0b]);
    assert_eq!(read(memory, GATE, 4096), expected_gates);
    let mut expected_data = input[pe::DATA_RAW..pe::DATA_RAW + 512].to_vec();
    pe::put32(&mut expected_data, (IAT - 0x3000) as usize, GATE + 80);
    pe::put32(&mut expected_data, (IAT - 0x3000) as usize + 4, GATE + 64);
    assert_eq!(read(memory, pe::BASE + 0x3000, 512), expected_data);
    assert_eq!(
        read(memory, pe::BASE + ILT, 12),
        input[data_file(ILT)..data_file(ILT) + 12]
    );
}

#[test]
fn named_virtual_free_uses_sixth_physical_slot_in_loader_and_native_process() {
    let bytes = named_free_and_alloc_image();
    let original = bytes.clone();
    let (memory, linked) = loader::load_pe32_linked_v5_at(&bytes, pe::BASE, GATE, 5)
        .unwrap()
        .into_parts();
    assert_named_link(&memory, linked, &original);
    assert_eq!(bytes, original);
    let mut engine = EngineInstance::new(5, KEY).unwrap();
    let arena = engine.arena().to_vec();
    let linked = engine
        .load_pe32_linked_v5_at(&bytes, pe::BASE, GATE)
        .unwrap();
    assert_named_link(engine.memory().unwrap(), linked, &original);
    assert_eq!(engine.arena(), arena);
    assert_eq!(bytes, original);
}

use ring3_engine::{
    abi::arena::CANCEL_OFFSET,
    loader::LoadError,
    memory::{Access, FaultReason, MemoryError, MemoryFault},
    process::HostError,
};

const V5_DATA_RAW: usize = 0x1000;
const V5_NAMES: [u32; 6] = [0x3100, 0x3120, 0x3140, 0x3160, 0x3180, 0x31a0];
const V5_IDS: [u32; 6] = [
    0x0001_0001,
    0x0001_0002,
    0x0001_0003,
    0x0001_0004,
    0x0001_0005,
    0x0001_0006,
];

fn v5_file(rva: u32) -> usize {
    if rva < 0x3000 {
        pe::TEXT_RAW + (rva - 0x1000) as usize
    } else {
        V5_DATA_RAW + (rva - 0x3000) as usize
    }
}

fn v5_fixup(bytes: &mut [u8], target: u32) {
    directory(bytes, 5, 0x31c0, 12);
    pe::put32(bytes, v5_file(0x31c0), target & !0xfff);
    pe::put32(bytes, v5_file(0x31c0) + 4, 12);
    pe::put16(bytes, v5_file(0x31c0) + 8, 0x3000 | (target & 0xfff) as u16);
    pe::put16(bytes, v5_file(0x31c0) + 10, 0);
}

fn v5_image(order: &[usize]) -> Vec<u8> {
    // pe headers come from the independent basic fixture; imports are authored here.
    let original = pe::image();
    let mut bytes = vec![0; 0x1200];
    bytes[..0x400].copy_from_slice(&original[..0x400]);
    pe::put16(&mut bytes, pe::COFF + 18, 0x0102);
    pe::put32(&mut bytes, pe::section(0) + 8, 0x200);
    pe::put32(&mut bytes, pe::section(1) + 20, V5_DATA_RAW as u32);
    pe::put32(&mut bytes, pe::section(1) + 36, 0x4000_0040);
    bytes[pe::TEXT_RAW..pe::TEXT_RAW + 512].fill(0x90);
    pe::put32(&mut bytes, pe::TEXT_RAW + 0x21, 0x0040_5000);
    directory(&mut bytes, 1, 0x3000, 40);
    directory(&mut bytes, 12, IAT, 4 * (order.len() as u32 + 1));
    for (field, value) in [(0, ILT), (12, MODULE), (16, IAT)] {
        pe::put32(&mut bytes, V5_DATA_RAW + field, value);
    }
    for (index, &symbol) in order.iter().enumerate() {
        pe::put32(&mut bytes, v5_file(ILT) + index * 4, V5_NAMES[symbol]);
        pe::put32(&mut bytes, v5_file(IAT) + index * 4, V5_NAMES[symbol]);
    }
    bytes[v5_file(MODULE)..v5_file(MODULE) + 13].copy_from_slice(b"KeRnEl32.dLl\0");
    for (index, symbol) in [
        b"GetLastError\0".as_slice(),
        b"SetLastError\0",
        b"ExitProcess\0",
        b"GetModuleHandleA\0",
        b"VirtualAlloc\0",
        b"VirtualFree\0",
    ]
    .into_iter()
    .enumerate()
    {
        pe::put16(&mut bytes, v5_file(V5_NAMES[index]), 0x1200 + index as u16);
        let at = v5_file(V5_NAMES[index]) + 2;
        bytes[at..at + symbol.len()].copy_from_slice(symbol);
    }
    v5_fixup(&mut bytes, 0x1021);
    bytes
}

fn v5_expected(base: u32, gate: u32, order: &[usize]) -> LinkedImageMetadata32V5 {
    let mut gates = [GateSpec {
        entry: GuestAddress(0),
        id: 0,
    }; 6];
    let mut count = 0;
    for (slot, &id) in V5_IDS.iter().enumerate() {
        if order.contains(&slot) {
            gates[count] = GateSpec {
                entry: GuestAddress(gate + slot as u32 * 16),
                id,
            };
            count += 1;
        }
    }
    LinkedImageMetadata32V5 {
        image: ImageMetadata32 {
            image_base: base,
            image_size: 0x7000,
            entry_point: base + 0x1000,
            mapped_pages: 5,
        },
        gate_base: gate,
        gate_count: count as u32,
        gates,
    }
}

fn v5_assert_image(
    memory: &AddressSpace,
    linked: LinkedImageMetadata32V5,
    source: &[u8],
    base: u32,
    gate: u32,
    order: &[usize],
) {
    assert_eq!(linked, v5_expected(base, gate, order));
    assert_eq!(memory.mapped_pages(), 5);
    assert_eq!(read(memory, base, 512), source[..512]);
    let mut text = source[pe::TEXT_RAW..pe::TEXT_RAW + 512].to_vec();
    pe::put32(&mut text, 0x21, base + 0x5000);
    assert_eq!(read(memory, base + 0x1000, 512), text);
    let mut data = source[V5_DATA_RAW..V5_DATA_RAW + 512].to_vec();
    for (index, &slot) in order.iter().enumerate() {
        pe::put32(
            &mut data,
            (IAT - 0x3000) as usize + index * 4,
            gate + slot as u32 * 16,
        );
    }
    assert_eq!(read(memory, base + 0x3000, 512), data);
    assert_eq!(
        read(memory, base + ILT, (order.len() + 1) * 4),
        source[v5_file(ILT)..v5_file(ILT) + (order.len() + 1) * 4]
    );
    assert_eq!(read(memory, base + 0x3200, 3584), vec![0; 3584]);
    assert_eq!(read(memory, base + 0x5000, 4096), vec![0; 4096]);
    let mut gates = vec![0; 4096];
    for slot in 0..6 {
        if order.contains(&slot) {
            gates[slot * 16..slot * 16 + 2].copy_from_slice(&[0x0f, 0x0b]);
        }
    }
    assert_eq!(read(memory, gate, 4096), gates);
}

fn v5_stage(bytes: &[u8], pages: u32) -> EngineInstance {
    let mut engine = EngineInstance::new(pages, KEY).unwrap();
    engine.begin_image_input(bytes.len() as u32).unwrap();
    for (index, chunk) in bytes.chunks(4096).enumerate() {
        engine
            .append_image_input(index as u32 * 4096, chunk)
            .unwrap();
    }
    engine
}

fn v5_permission(address: u32, access: Access) -> MemoryError {
    MemoryError::Fault(MemoryFault {
        address: GuestAddress(address),
        access,
        reason: FaultReason::Permission,
    })
}

#[test]
fn every_free_bearing_subset_packs_six_slots_and_patches_literal_iat() {
    for mask in 32..64 {
        let order: Vec<usize> = (0..6).filter(|slot| mask & (1 << slot) != 0).collect();
        let bytes = v5_image(&order);
        let original = bytes.clone();
        let (memory, linked) = loader::load_pe32_linked_v5_at(&bytes, 0x0040_0000, GATE, 5)
            .unwrap()
            .into_parts();
        v5_assert_image(&memory, linked, &original, 0x0040_0000, GATE, &order);
        assert_eq!(bytes, original);
    }
}

#[test]
fn six_symbol_reordering_and_eight_duplicate_free_slots_own_relocated_bytes() {
    for order in [&[5, 4, 3, 1, 0, 2, 5, 4][..], &[5; 8][..]] {
        for base in [0x0040_0000, 0x0050_0000] {
            let mut bytes = v5_image(order);
            directory(&mut bytes, 12, 0, 0);
            let original = bytes.clone();
            let loaded = loader::load_pe32_linked_v5_at(&bytes, base, GATE, 5).unwrap();
            bytes.fill(0xee);
            let (memory, linked) = loaded.into_parts();
            v5_assert_image(&memory, linked, &original, base, GATE, order);
        }
    }
}

#[test]
fn free_gate_page_boundaries_and_permissions_stay_exact() {
    let bytes = v5_image(&[5]);
    for gate in [GATE, 0xffff_f000, 0x003f_f000, 0x0040_7000] {
        let (mut memory, linked) = loader::load_pe32_linked_v5_at(&bytes, 0x0040_0000, gate, 5)
            .unwrap()
            .into_parts();
        v5_assert_image(&memory, linked, &bytes, 0x0040_0000, gate, &[5]);
        for offset in [0, 80, 81, 4095] {
            assert_eq!(
                memory.write(GuestAddress(gate + offset), &[0xff]),
                Err(v5_permission(gate + offset, Access::Write))
            );
        }
        memory.fetch(GuestAddress(gate + 80), &mut [0; 2]).unwrap();
        assert_eq!(
            memory.write(GuestAddress(0x0040_3080), &[0xff]),
            Err(v5_permission(0x0040_3080, Access::Write))
        );
        assert_eq!(
            memory.fetch(GuestAddress(0x0040_3080), &mut [0; 1]),
            Err(v5_permission(0x0040_3080, Access::Execute))
        );
    }
    for gate in [0, GATE + 1, 0x0040_0000, 0x0040_2000, 0x0040_6000] {
        assert_eq!(
            loader::load_pe32_linked_v5_at(&bytes, 0x0040_0000, gate, 5).err(),
            Some(LoadError::Malformed)
        );
    }
}

fn v5_refused_image(mode: u32) -> (Vec<u8>, LoadError) {
    let mut bytes = v5_image(&[5]);
    let error = match mode {
        0 => {
            bytes[v5_file(V5_NAMES[5]) + 2] = b'v';
            LoadError::Unsupported
        }
        1 => {
            bytes[v5_file(MODULE)] = b'u';
            LoadError::Unsupported
        }
        2 => {
            for table in [ILT, IAT] {
                pe::put32(&mut bytes, v5_file(table), 0x8000_0001);
            }
            LoadError::Unsupported
        }
        3 => {
            pe::put32(&mut bytes, v5_file(IAT), V5_NAMES[4]);
            LoadError::Malformed
        }
        4 => {
            pe::put32(&mut bytes, V5_DATA_RAW + 12, V5_NAMES[5] + 2);
            LoadError::Malformed
        }
        5 => {
            v5_fixup(&mut bytes, IAT);
            LoadError::Malformed
        }
        6 => {
            v5_fixup(&mut bytes, ILT);
            LoadError::Malformed
        }
        7 => {
            for table in [ILT, IAT] {
                pe::put32(&mut bytes, v5_file(table), 0x3200);
            }
            LoadError::Malformed
        }
        _ => panic!("bounded refusal mode"),
    };
    (bytes, error)
}

#[test]
fn new_profile_refuses_unsupported_alias_and_capacity_inputs_without_publication() {
    for mode in 0..8 {
        let (bytes, error) = v5_refused_image(mode);
        let original = bytes.clone();
        assert_eq!(
            loader::load_pe32_linked_v5_at(&bytes, 0x0040_0000, GATE, 5).err(),
            Some(error)
        );
        let mut engine = EngineInstance::new(5, KEY).unwrap();
        let arena = engine.arena().to_vec();
        assert_eq!(
            engine.load_pe32_linked_v5_at(&bytes, 0x0040_0000, GATE),
            Err(HostError::Loader(error))
        );
        assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
        assert_eq!(engine.arena(), arena);
        assert_eq!(bytes, original);
    }
    for (order, pages) in [(vec![5; 9], 5), (vec![0, 1, 2, 3, 4, 5], 4)] {
        let bytes = v5_image(&order);
        assert_eq!(
            loader::load_pe32_linked_v5_at(&bytes, 0x0040_0000, GATE, pages).err(),
            Some(LoadError::Capacity)
        );
        let mut engine = v5_stage(&bytes, pages);
        let arena = engine.arena().to_vec();
        assert_eq!(
            engine.load_pe32_linked_v5_input_at(0x0040_0000, GATE),
            Err(HostError::Loader(LoadError::Capacity))
        );
        assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
        assert_eq!(engine.arena(), arena);
        assert_eq!(engine.begin_image_input(1), Err(HostError::InvalidRequest));
        engine.abort_image_input().unwrap();
        assert_eq!(engine.arena(), arena);
    }
}

#[test]
fn copied_free_input_retries_from_owned_prefix_and_abort_preserves_published_image() {
    let source = v5_image(&[5, 4, 2]);
    let mut engine = EngineInstance::new(5, KEY).unwrap();
    engine.arena_mut().unwrap().fill(0xa5);
    engine.arena_mut().unwrap()[CANCEL_OFFSET..CANCEL_OFFSET + 4]
        .copy_from_slice(&1_u32.to_le_bytes());
    let arena = engine.arena().to_vec();
    engine.begin_image_input(source.len() as u32).unwrap();
    let mut first = source[..4096].to_vec();
    engine.append_image_input(0, &first).unwrap();
    first.fill(0xee);
    assert_eq!(
        engine.load_pe32_linked_v5_input_at(0, 0),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
    assert_eq!(engine.arena(), arena);
    engine.map(0x9000, 1, 3).unwrap();
    engine.write32(0x9000, 0xdead_beef).unwrap();
    // host setup wrote its Helper receipt; restore the frozen arena before admission.
    engine.arena_mut().unwrap().copy_from_slice(&arena);
    assert_eq!(
        engine.load_pe32_linked_v5_input_at(0, 0),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(
        read(engine.memory().unwrap(), 0x9000, 4),
        0xdead_beef_u32.to_le_bytes()
    );
    assert_eq!(engine.arena(), arena);
    engine.unmap(0x9000, 1).unwrap();
    let mut last = source[4096..].to_vec();
    engine.append_image_input(4096, &last).unwrap();
    last.fill(0xee);
    assert_eq!(
        engine.load_pe32_linked_v4_input_at(0x0040_0000, GATE),
        Err(HostError::Loader(LoadError::Unsupported))
    );
    assert_eq!(engine.arena(), arena);
    assert_eq!(
        engine.load_pe32_linked_v5_input_at(0x0040_0000, 0x0040_2000),
        Err(HostError::Loader(LoadError::Malformed))
    );
    assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
    assert_eq!(engine.arena(), arena);
    let linked = engine
        .load_pe32_linked_v5_input_at(0x0050_0000, GATE)
        .unwrap();
    v5_assert_image(
        engine.memory().unwrap(),
        linked,
        &source,
        0x0050_0000,
        GATE,
        &[5, 4, 2],
    );
    assert_eq!(engine.arena(), arena);
    assert_eq!(
        engine.append_image_input(source.len() as u32, b"x"),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(
        engine.load_pe32_linked_v5_input_at(0, 0),
        Err(HostError::InvalidRequest)
    );
    engine.abort_image_input().unwrap();
    v5_assert_image(
        engine.memory().unwrap(),
        linked,
        &source,
        0x0050_0000,
        GATE,
        &[5, 4, 2],
    );
    assert_eq!(engine.arena(), arena);
    engine.close();
    assert_eq!(
        engine.load_pe32_linked_v5_at(b"bad", 0, 0),
        Err(HostError::Closed)
    );
    assert_eq!(
        engine.load_pe32_linked_v5_input_at(0, 0),
        Err(HostError::Closed)
    );
    assert_eq!(engine.arena(), arena);
}

use ring3_engine::{
    abi::{
        arena::TRANSFER_OFFSET,
        x86::{decode_state, encode_exit_v3, encode_state},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    windows::CallingConvention32,
};

fn allocate_from_loaded_image(engine: &mut EngineInstance, generation: u32, return_pc: u32) -> u32 {
    let esp = 0x8080;
    for (index, value) in [return_pc, 0, 1, 0x3000, 4].into_iter().enumerate() {
        engine.write32(esp + index as u32 * 4, value).unwrap();
    }
    let state = State32 {
        registers: [
            0x89ab_cdef,
            0x1357_9bdf,
            0x2345_6789,
            0x3456_789a,
            esp,
            0x5678_9abc,
            0x6789_abcd,
            0x789a_bcde,
        ],
        eip: GATE + 64,
        eflags: 0xcd7,
    };
    let arena = engine.arena_mut().unwrap();
    encode_state(&state, &mut arena[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id: 0x0001_0005 },
        },
        &mut arena[56..96],
    )
    .unwrap();
    arena[CANCEL_OFFSET..CANCEL_OFFSET + 4].fill(0);
    let call = engine
        .capture_call(KEY, generation, CallingConvention32::Stdcall, 4)
        .unwrap();
    assert_eq!(&call.arguments[..4], &[0, 1, 0x3000, 4]);
    engine
        .complete_windows_call(KEY, generation, call.token)
        .unwrap();
    let completed = decode_state(&engine.arena()[..56]).unwrap();
    let address = completed.registers[0];
    let mut expected = state;
    expected.registers[0] = address;
    expected.registers[4] = esp + 20;
    expected.eip = return_pc;
    assert_eq!(completed, expected);
    address
}

#[test]
fn named_virtual_alloc_excludes_the_loaded_image_extent_and_preserves_sparse_gaps() {
    let cases: [(u32, u32, u32, u32, &[u32]); 7] = [
        (0x0040_0000, 0x7000, 0x5000, 0x2000, &[0x1000_0000]),
        (0x1000_0000, 0x20000, 0x5000, 0x10000, &[0x1002_0000]),
        (0x1000_0000, 0x21000, 0x5000, 0x10000, &[0x1003_0000]),
        (
            0x1001_0000,
            0x20000,
            0x5000,
            0x10000,
            &[0x1000_0000, 0x1003_0000],
        ),
        (0x1000_0000, 0x40000, 0x30000, 0x10000, &[0x1004_0000]),
        (0x0fff_0000, 0x30000, 0x5000, 0x10000, &[0x1002_0000]),
        (0xffff_0000, 0x10000, 0x5000, 0x2000, &[0x1000_0000]),
    ];
    for (base, image_size, bss_rva, gap_rva, allocations) in cases {
        let mut bytes = named_free_and_alloc_image();
        pe::put32(&mut bytes, pe::OPTIONAL + 28, base);
        pe::put32(&mut bytes, pe::OPTIONAL + 56, image_size);
        pe::put32(&mut bytes, pe::section(2) + 12, bss_rva);
        let mut engine = EngineInstance::new(9, KEY).unwrap();
        let linked = engine.load_pe32_linked_v5_at(&bytes, base, GATE).unwrap();
        assert_eq!(linked.image.image_base, base);
        assert_eq!(linked.image.image_size, image_size);
        assert_eq!(linked.image.mapped_pages, 5);
        assert_eq!(linked.gate_count, 2);
        assert_eq!(linked.gates[0].entry, GuestAddress(GATE + 64));
        assert_eq!(linked.gates[0].id, 0x0001_0005);
        let gap = base + gap_rva;
        assert!(matches!(
            engine.memory().unwrap().resolve(GuestAddress(gap), Access::Read),
            Err(MemoryError::Fault(fault))
                if fault.address == GuestAddress(gap)
                    && fault.access == Access::Read
                    && fault.reason == FaultReason::Unmapped
        ));
        assert_eq!(
            engine.start_loaded_image(gap, 1),
            Err(HostError::InvalidRequest)
        );
        engine.start_loaded_image(0x8000, 1).unwrap();
        for (index, value) in [GATE + 64, 2, GATE + 64, 0x0001_0005]
            .into_iter()
            .enumerate()
        {
            let at = TRANSFER_OFFSET + index * 4;
            engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        let resident = engine.compile_resident_with_gates(1, 1).unwrap().get();
        let generation = engine.compile_with_gates(1, 1).unwrap();
        let resident_bytes = engine.resident_bytes(resident).unwrap().to_vec();
        let artifact_bytes = engine.artifact_bytes().unwrap().to_vec();
        let samples = [
            (base, 512),
            (base + 0x1000, 512),
            (base + 0x3000, 512),
            (base + bss_rva, 4096),
        ];
        let image_bytes =
            samples.map(|(address, length)| read(engine.memory().unwrap(), address, length));
        let image_code = engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(base + 0x1000), 16)
            .unwrap();
        for (index, &expected_address) in allocations.iter().enumerate() {
            let address = allocate_from_loaded_image(&mut engine, generation, base + 0x1000);
            assert_eq!(
                address, expected_address,
                "base={base:08x}, image_size={image_size:x}, allocation={index}"
            );
            assert_eq!(engine.memory().unwrap().mapped_pages(), 7 + index as u32);
            assert_eq!(read(engine.memory().unwrap(), address, 4096), vec![0; 4096]);
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(address), Access::Write)
                    .is_ok()
            );
            assert!(matches!(
                engine.memory().unwrap().resolve(GuestAddress(address), Access::Execute),
                Err(MemoryError::Fault(fault)) if fault.reason == FaultReason::Permission
            ));
            engine.write32(address + 4092, 0xf123_4567).unwrap();
            assert_eq!(
                read(engine.memory().unwrap(), address + 4092, 4),
                0xf123_4567_u32.to_le_bytes()
            );
            for &previous in &allocations[..=index] {
                assert_eq!(
                    read(engine.memory().unwrap(), previous + 4092, 4),
                    0xf123_4567_u32.to_le_bytes()
                );
            }
            assert!(matches!(
                engine.memory().unwrap().resolve(GuestAddress(gap), Access::Read),
                Err(MemoryError::Fault(fault))
                    if fault.address == GuestAddress(gap)
                        && fault.reason == FaultReason::Unmapped
            ));
            for ((sample, length), expected) in samples.into_iter().zip(&image_bytes) {
                assert_eq!(&read(engine.memory().unwrap(), sample, length), expected);
            }
            assert!(engine.memory().unwrap().is_code_current(&image_code));
            assert_eq!(engine.generation(), generation);
            assert_eq!(engine.resident_bytes(resident).unwrap(), resident_bytes);
            assert_eq!(engine.artifact_bytes().unwrap(), artifact_bytes);
        }
    }
}
