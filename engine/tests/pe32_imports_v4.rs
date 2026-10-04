#[allow(dead_code)]
#[path = "support/pe32.rs"]
mod pe;

use ring3_engine::{
    abi::arena::CANCEL_OFFSET,
    cpu::dbt::GateSpec,
    loader::{self, ImageMetadata32, LinkedImageMetadata32V4, LoadError},
    memory::{Access, AddressSpace, FaultReason, GuestAddress, MemoryError, MemoryFault},
    process::{EngineInstance, HostError},
};

const KEY: u64 = 0xe359_ffff_1234_5678;
const GATE: u32 = 0x7000_0000;
const DATA_RAW: usize = 0x1000;
const ILT: u32 = 0x3040;
const IAT: u32 = 0x3080;
const MODULE: u32 = 0x30c0;
const NAMES: [u32; 5] = [0x3100, 0x3120, 0x3140, 0x3160, 0x3180];
const IDS: [u32; 5] = [
    0x0001_0001,
    0x0001_0002,
    0x0001_0003,
    0x0001_0004,
    0x0001_0005,
];

fn file(rva: u32) -> usize {
    if rva < 0x3000 {
        pe::TEXT_RAW + (rva - 0x1000) as usize
    } else {
        DATA_RAW + (rva - 0x3000) as usize
    }
}

fn directory(bytes: &mut [u8], index: usize, rva: u32, size: u32) {
    pe::put32(bytes, pe::OPTIONAL + 96 + index * 8, rva);
    pe::put32(bytes, pe::OPTIONAL + 100 + index * 8, size);
}

fn relocation(bytes: &mut [u8], target: u32) {
    directory(bytes, 5, 0x31c0, 12);
    pe::put32(bytes, file(0x31c0), target & !0xfff);
    pe::put32(bytes, file(0x31c0) + 4, 12);
    pe::put16(bytes, file(0x31c0) + 8, 0x3000 | (target & 0xfff) as u16);
    pe::put16(bytes, file(0x31c0) + 10, 0);
}

fn image(names: &[u32]) -> Vec<u8> {
    // literal imports use only the independent basic pe header builder.
    let original = pe::image();
    let mut bytes = vec![0; 0x1200];
    bytes[..0x400].copy_from_slice(&original[..0x400]);
    pe::put16(&mut bytes, pe::COFF + 18, 0x0102);
    pe::put32(&mut bytes, pe::section(0) + 8, 0x200);
    pe::put32(&mut bytes, pe::section(1) + 20, DATA_RAW as u32);
    pe::put32(&mut bytes, pe::section(1) + 36, 0x4000_0040);
    bytes[pe::TEXT_RAW..pe::TEXT_RAW + 512].fill(0x90);
    pe::put32(&mut bytes, pe::TEXT_RAW + 0x21, pe::BASE + 0x5000);
    directory(&mut bytes, 1, 0x3000, 40);
    directory(&mut bytes, 12, IAT, 4 * (names.len() as u32 + 1));
    for (field, value) in [(0, ILT), (12, MODULE), (16, IAT)] {
        pe::put32(&mut bytes, DATA_RAW + field, value);
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
        b"GetModuleHandleA\0",
        b"VirtualAlloc\0",
    ]
    .into_iter()
    .enumerate()
    {
        pe::put16(&mut bytes, file(NAMES[index]), 0x1234 + index as u16);
        let at = file(NAMES[index]) + 2;
        bytes[at..at + symbol.len()].copy_from_slice(symbol);
    }
    relocation(&mut bytes, 0x1021);
    bytes
}

fn read(memory: &AddressSpace, address: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    memory.read(GuestAddress(address), &mut bytes).unwrap();
    bytes
}

fn word(memory: &AddressSpace, address: u32) -> u32 {
    u32::from_le_bytes(read(memory, address, 4).try_into().unwrap())
}

fn expected(base: u32, gate: u32, names: &[u32]) -> LinkedImageMetadata32V4 {
    let mut gates = [GateSpec {
        entry: GuestAddress(0),
        id: 0,
    }; 5];
    let mut count = 0;
    for index in 0..5 {
        if names.contains(&NAMES[index]) {
            gates[count] = GateSpec {
                entry: GuestAddress(gate + index as u32 * 16),
                id: IDS[index],
            };
            count += 1;
        }
    }
    LinkedImageMetadata32V4 {
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

fn gate_page(names: &[u32]) -> Vec<u8> {
    let mut page = vec![0; 4096];
    for index in 0..5 {
        if names.contains(&NAMES[index]) {
            page[index * 16..index * 16 + 2].copy_from_slice(&[0x0f, 0x0b]);
        }
    }
    page
}

fn permission(address: u32, access: Access) -> MemoryError {
    MemoryError::Fault(MemoryFault {
        address: GuestAddress(address),
        access,
        reason: FaultReason::Permission,
    })
}

fn staged(bytes: &[u8], pages: u32) -> EngineInstance {
    let mut engine = EngineInstance::new(pages, KEY).unwrap();
    engine.begin_image_input(bytes.len() as u32).unwrap();
    for (index, chunk) in bytes.chunks(4096).enumerate() {
        engine
            .append_image_input(index as u32 * 4096, chunk)
            .unwrap();
    }
    engine
}

#[test]
fn allocation_subsets_pack_metadata_and_keep_the_fifth_physical_slot() {
    for mask in 16..32 {
        let names: Vec<u32> = NAMES
            .into_iter()
            .enumerate()
            .filter_map(|(index, name)| (mask & (1 << index) != 0).then_some(name))
            .collect();
        let bytes = image(&names);
        let original = bytes.clone();
        let (memory, linked) = loader::load_pe32_linked_v4_at(&bytes, pe::BASE, GATE, 5)
            .unwrap()
            .into_parts();
        assert_eq!(linked, expected(pe::BASE, GATE, &names));
        assert_eq!(memory.mapped_pages(), 5);
        assert_eq!(read(&memory, GATE, 4096), gate_page(&names));
        for (index, &name) in names.iter().enumerate() {
            let slot = NAMES.iter().position(|&value| value == name).unwrap() as u32;
            assert_eq!(
                word(&memory, pe::BASE + IAT + index as u32 * 4),
                GATE + slot * 16
            );
            assert_eq!(word(&memory, pe::BASE + ILT + index as u32 * 4), name);
        }
        assert_eq!(word(&memory, pe::BASE + IAT + names.len() as u32 * 4), 0);
        assert_eq!(bytes, original);
    }
}

#[test]
fn duplicates_and_reordering_keep_slots_at_two_bases_and_own_late_source_bytes() {
    for names in [
        &[NAMES[4], NAMES[3], NAMES[1], NAMES[0], NAMES[2], NAMES[4]][..],
        &[NAMES[4]; 8][..],
    ] {
        for base in [pe::BASE, 0x0050_0000] {
            let (memory, linked) = {
                let mut bytes = image(names);
                directory(&mut bytes, 12, 0, 0);
                let loaded = loader::load_pe32_linked_v4_at(&bytes, base, GATE, 5).unwrap();
                bytes.fill(0xa5);
                loaded.into_parts()
            };
            assert_eq!(linked, expected(base, GATE, names));
            assert_eq!(word(&memory, base + 0x1021), base + 0x5000);
            assert_eq!(word(&memory, base + pe::OPTIONAL as u32 + 28), pe::BASE);
            for (index, &name) in names.iter().enumerate() {
                let slot = NAMES.iter().position(|&value| value == name).unwrap() as u32;
                assert_eq!(
                    word(&memory, base + IAT + index as u32 * 4),
                    GATE + slot * 16
                );
            }
            assert_eq!(read(&memory, base + NAMES[4] + 2, 13), b"VirtualAlloc\0");
        }
    }
}

#[test]
fn fifth_gate_is_rx_and_full_page_bounds_and_iat_permissions_remain_exact() {
    let bytes = image(&[NAMES[4]]);
    for gate in [
        GATE,
        0xffff_f000,
        pe::BASE - 4096,
        pe::BASE + pe::IMAGE_SIZE,
    ] {
        let (mut memory, linked) = loader::load_pe32_linked_v4_at(&bytes, pe::BASE, gate, 5)
            .unwrap()
            .into_parts();
        assert_eq!(linked, expected(pe::BASE, gate, &[NAMES[4]]));
        assert_eq!(read(&memory, gate, 4096), gate_page(&[NAMES[4]]));
        for offset in [0, 64, 65, 4095] {
            assert_eq!(
                memory.write(GuestAddress(gate + offset), &[0xff]),
                Err(permission(gate + offset, Access::Write))
            );
        }
        memory.fetch(GuestAddress(gate + 64), &mut [0; 2]).unwrap();
        assert_eq!(
            memory.write(GuestAddress(pe::BASE + IAT), &[0xff]),
            Err(permission(pe::BASE + IAT, Access::Write))
        );
        assert_eq!(
            memory.fetch(GuestAddress(pe::BASE + IAT), &mut [0; 1]),
            Err(permission(pe::BASE + IAT, Access::Execute))
        );
    }
    for gate in [0, GATE + 1, pe::BASE, pe::BASE + 0x2000, pe::BASE + 0x6000] {
        assert_eq!(
            loader::load_pe32_linked_v4_at(&bytes, pe::BASE, gate, 5).err(),
            Some(LoadError::Malformed)
        );
    }
}

#[test]
fn old_profiles_refuse_allocation_keep_arrays_and_preserve_staging_for_v4_retry() {
    let bytes = image(&[NAMES[4]]);
    assert_eq!(
        loader::load_pe32_linked_at(&bytes, pe::BASE, GATE, 5).err(),
        Some(LoadError::Unsupported)
    );
    assert_eq!(
        loader::load_pe32_linked_v2_at(&bytes, pe::BASE, GATE, 5).err(),
        Some(LoadError::Unsupported)
    );
    assert_eq!(
        loader::load_pe32_linked_v3_at(&bytes, pe::BASE, GATE, 5).err(),
        Some(LoadError::Unsupported)
    );
    let (_, old): (_, loader::LinkedImageMetadata32) =
        loader::load_pe32_linked_at(&image(&NAMES[..2]), pe::BASE, GATE, 5)
            .unwrap()
            .into_parts();
    let (_, v2): (_, loader::LinkedImageMetadata32V2) =
        loader::load_pe32_linked_v2_at(&image(&NAMES[..3]), pe::BASE, GATE, 5)
            .unwrap()
            .into_parts();
    let (_, v3): (_, loader::LinkedImageMetadata32V3) =
        loader::load_pe32_linked_v3_at(&image(&NAMES[..4]), pe::BASE, GATE, 5)
            .unwrap()
            .into_parts();
    assert_eq!(old.gates.len(), 2);
    assert_eq!(v2.gates.len(), 3);
    assert_eq!(v3.gates.len(), 4);
    assert_eq!(old.gates, expected(pe::BASE, GATE, &NAMES[..2]).gates[..2]);
    assert_eq!(v2.gates, expected(pe::BASE, GATE, &NAMES[..3]).gates[..3]);
    assert_eq!(v3.gates, expected(pe::BASE, GATE, &NAMES[..4]).gates[..4]);
    let mut engine = staged(&bytes, 5);
    let arena = engine.arena().to_vec();
    assert_eq!(
        engine.load_pe32_linked_v2_input_at(pe::BASE, GATE),
        Err(HostError::Loader(LoadError::Unsupported))
    );
    assert_eq!(
        engine.load_pe32_linked_v3_input_at(pe::BASE, GATE),
        Err(HostError::Loader(LoadError::Unsupported))
    );
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
    assert_eq!(
        engine.load_pe32_linked_v4_input_at(pe::BASE, GATE).unwrap(),
        expected(pe::BASE, GATE, &[NAMES[4]])
    );
}

#[test]
fn new_profile_keeps_structural_name_and_capacity_failures_closed() {
    for mode in 0..5 {
        let mut bytes = image(&[NAMES[4]]);
        let error = match mode {
            0 => {
                bytes[file(NAMES[4]) + 2] = b'v';
                LoadError::Unsupported
            }
            1 => {
                bytes[file(MODULE)] = b'u';
                LoadError::Unsupported
            }
            2 => {
                for table in [ILT, IAT] {
                    pe::put32(&mut bytes, file(table), 0x8000_0001);
                }
                LoadError::Unsupported
            }
            3 => {
                pe::put32(&mut bytes, DATA_RAW + 12, NAMES[4] + 2);
                LoadError::Malformed
            }
            _ => {
                relocation(&mut bytes, IAT);
                LoadError::Malformed
            }
        };
        assert_eq!(
            loader::load_pe32_linked_v4_at(&bytes, pe::BASE, GATE, 5).err(),
            Some(error)
        );
    }
    for names in [vec![0x3200], vec![NAMES[4]; 9]] {
        let error = if names.len() == 9 {
            LoadError::Capacity
        } else {
            LoadError::Malformed
        };
        assert_eq!(
            loader::load_pe32_linked_v4_at(&image(&names), pe::BASE, GATE, 5).err(),
            Some(error)
        );
    }
    assert_eq!(
        loader::load_pe32_linked_v4_at(&image(&NAMES), pe::BASE, GATE, 4).err(),
        Some(LoadError::Capacity)
    );
}

#[test]
fn native_and_copied_failures_preserve_arena_and_success_consumes_only_complete_input() {
    let bytes = image(&NAMES);
    for copied in [false, true] {
        let mut engine = if copied {
            staged(&bytes, 5)
        } else {
            EngineInstance::new(5, KEY).unwrap()
        };
        engine.arena_mut().unwrap().fill(0xa5);
        engine.arena_mut().unwrap()[CANCEL_OFFSET..CANCEL_OFFSET + 4]
            .copy_from_slice(&1_u32.to_le_bytes());
        let arena = engine.arena().to_vec();
        let bad = if copied {
            engine.load_pe32_linked_v4_input_at(pe::BASE, pe::BASE + 0x2000)
        } else {
            engine.load_pe32_linked_v4_at(&bytes, pe::BASE, pe::BASE + 0x2000)
        };
        assert_eq!(bad, Err(HostError::Loader(LoadError::Malformed)));
        assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
        assert_eq!(engine.arena(), arena);
        let linked = if copied {
            engine.load_pe32_linked_v4_input_at(0x0050_0000, GATE)
        } else {
            engine.load_pe32_linked_v4_at(&bytes, 0x0050_0000, GATE)
        }
        .unwrap();
        assert_eq!(linked, expected(0x0050_0000, GATE, &NAMES));
        assert_eq!(engine.arena(), arena);
        assert_eq!(
            word(engine.memory().unwrap(), 0x0050_0000 + 0x1021),
            0x0050_5000
        );
        assert_eq!(
            engine.load_pe32_linked_v4_input_at(0, 0),
            Err(HostError::InvalidRequest)
        );
        engine.abort_image_input().unwrap();
        engine.close();
        assert_eq!(
            engine.load_pe32_linked_v4_at(b"bad", 0, 0),
            Err(HostError::Closed)
        );
        assert_eq!(
            engine.load_pe32_linked_v4_input_at(0, 0),
            Err(HostError::Closed)
        );
        assert_eq!(engine.arena(), arena);
    }
    let mut incomplete = EngineInstance::new(5, KEY).unwrap();
    incomplete.begin_image_input(bytes.len() as u32).unwrap();
    incomplete.append_image_input(0, &bytes[..4096]).unwrap();
    let arena = incomplete.arena().to_vec();
    assert_eq!(
        incomplete.load_pe32_linked_v4_input_at(0, 0),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(incomplete.arena(), arena);
    assert_eq!(incomplete.memory().unwrap().mapped_pages(), 0);
    incomplete.append_image_input(4096, &bytes[4096..]).unwrap();
    assert_eq!(
        incomplete
            .load_pe32_linked_v4_input_at(pe::BASE, GATE)
            .unwrap(),
        expected(pe::BASE, GATE, &NAMES)
    );
}
