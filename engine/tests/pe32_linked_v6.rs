use ring3_engine::{
    cpu::dbt::GateSpec,
    loader::{self, LoadError},
    memory::{Access, GuestAddress},
    process::{EngineInstance, HostError},
};

#[path = "support/pe32.rs"]
#[allow(dead_code)]
mod pe;

const KEY: u64 = 0xf123_4567_89ab_cdef;
const BASE: u32 = 0x0040_0000;
const GATE: u32 = 0x7000_0000;
const NAMES: [&str; 7] = [
    "GetLastError",
    "SetLastError",
    "ExitProcess",
    "GetModuleHandleA",
    "VirtualAlloc",
    "VirtualFree",
    "VirtualProtect",
];

fn image(indices: &[usize]) -> Vec<u8> {
    let mut bytes = pe::image();
    bytes.resize(0xa00, 0);
    pe::put16(&mut bytes, pe::COFF + 18, 0x0102);
    pe::put32(&mut bytes, pe::OPTIONAL + 8, 1024);
    pe::put32(&mut bytes, pe::OPTIONAL + 12, 0);
    pe::put32(&mut bytes, pe::OPTIONAL + 56, 0x6000);
    for (index, rva, size) in [
        (1, 0x3000, 40),
        (5, 0x5000, 12),
        (12, 0x3080, 4 * (indices.len() as u32 + 1)),
    ] {
        pe::put32(&mut bytes, pe::OPTIONAL + 96 + index * 8, rva);
        pe::put32(&mut bytes, pe::OPTIONAL + 100 + index * 8, size);
    }
    pe::put32(&mut bytes, pe::section(0) + 8, 64);
    bytes[pe::TEXT_RAW..pe::TEXT_RAW + 512].fill(0xcc);
    bytes[pe::TEXT_RAW..pe::TEXT_RAW + 6].copy_from_slice(&[0xb8, 42, 0, 0, 0, 0xc3]);
    pe::put32(&mut bytes, pe::TEXT_RAW + 0x21, BASE + 0x3000);
    pe::put32(&mut bytes, pe::section(1) + 8, 1024);
    pe::put32(&mut bytes, pe::section(1) + 16, 1024);
    bytes[pe::DATA_RAW..0x800].fill(0);
    for (index, value) in [0x3040, 0, 0, 0x30c0, 0x3080].into_iter().enumerate() {
        pe::put32(&mut bytes, pe::DATA_RAW + index * 4, value);
    }
    for (index, &name) in indices.iter().enumerate() {
        for table in [0x440, 0x480] {
            pe::put32(&mut bytes, table + index * 4, 0x3100 + name as u32 * 32);
        }
    }
    bytes[0x4c0..0x4cd].copy_from_slice(b"KeRnEl32.dLl\0");
    for (index, name) in NAMES.into_iter().enumerate() {
        let at = 0x500 + index * 32;
        pe::put16(&mut bytes, at, 0x1234 + index as u16);
        bytes[at + 2..at + 2 + name.len()].copy_from_slice(name.as_bytes());
    }
    bytes[pe::section(2)..pe::section(2) + 8].copy_from_slice(b".fixups\0");
    for (offset, value) in [(8, 512), (16, 512), (20, 0x800), (36, 0x4000_0040)] {
        pe::put32(&mut bytes, pe::section(2) + offset, value);
    }
    pe::put32(&mut bytes, 0x800, 0x1000);
    pe::put32(&mut bytes, 0x804, 12);
    pe::put16(&mut bytes, 0x808, 0x3021);
    bytes
}

fn expected_gate(index: usize) -> GateSpec {
    GateSpec {
        entry: GuestAddress(GATE + index as u32 * 16),
        id: 0x10001 + index as u32,
    }
}

#[test]
fn v6_resolves_all_seven_names_in_api_order_at_both_image_bases() {
    let order = [6, 4, 0, 5, 2, 1, 3];
    let bytes = image(&order);
    for base in [BASE, 0x0050_0000] {
        let (memory, linked) = loader::load_pe32_linked_v6_at(&bytes, base, GATE, 6)
            .unwrap()
            .into_parts();
        assert_eq!(linked.image.image_base, base);
        assert_eq!(linked.image.image_size, 0x6000);
        assert_eq!(linked.image.entry_point, base + 0x1000);
        assert_eq!(linked.image.mapped_pages, 5);
        assert_eq!(linked.gate_base, GATE);
        assert_eq!(linked.gate_count, 7);
        assert_eq!(linked.gates, std::array::from_fn(expected_gate));
        let mut iat = [0; 32];
        memory.read(GuestAddress(base + 0x3080), &mut iat).unwrap();
        let mut expected = [0; 32];
        for (index, api) in order.into_iter().enumerate() {
            pe::put32(&mut expected, index * 4, GATE + api as u32 * 16);
        }
        assert_eq!(iat, expected);
        let mut relocated = [0; 4];
        memory
            .read(GuestAddress(base + 0x1021), &mut relocated)
            .unwrap();
        assert_eq!(relocated, (base + 0x3000).to_le_bytes());
        let mut gates = vec![0; 4096];
        memory.read(GuestAddress(GATE), &mut gates).unwrap();
        let mut expected = vec![0; 4096];
        for index in 0..7 {
            expected[index * 16..index * 16 + 2].copy_from_slice(&[0x0f, 0x0b]);
        }
        assert_eq!(gates, expected);
        assert!(
            memory
                .resolve(GuestAddress(GATE + 96), Access::Execute)
                .is_ok()
        );
        assert!(
            memory
                .resolve(GuestAddress(GATE + 96), Access::Write)
                .is_err()
        );
    }
}

#[test]
fn each_old_profile_rejects_the_new_named_api() {
    let bytes = image(&[6]);
    assert_eq!(
        loader::load_pe32_linked_at(&bytes, BASE, GATE, 6).err(),
        Some(LoadError::Unsupported)
    );
    assert_eq!(
        loader::load_pe32_linked_v2_at(&bytes, BASE, GATE, 6).err(),
        Some(LoadError::Unsupported)
    );
    assert_eq!(
        loader::load_pe32_linked_v3_at(&bytes, BASE, GATE, 6).err(),
        Some(LoadError::Unsupported)
    );
    assert_eq!(
        loader::load_pe32_linked_v4_at(&bytes, BASE, GATE, 6).err(),
        Some(LoadError::Unsupported)
    );
    assert_eq!(
        loader::load_pe32_linked_v5_at(&bytes, BASE, GATE, 6).err(),
        Some(LoadError::Unsupported)
    );
}

#[test]
fn old_profiles_keep_their_exact_gate_array_projections() {
    let v1 = loader::load_pe32_linked_at(&image(&[1, 0]), BASE, GATE, 6)
        .unwrap()
        .into_parts()
        .1;
    assert_eq!(v1.gates, [expected_gate(0), expected_gate(1)]);
    assert_eq!(v1.gate_count, 2);
    let v2 = loader::load_pe32_linked_v2_at(&image(&[2, 1, 0]), BASE, GATE, 6)
        .unwrap()
        .into_parts()
        .1;
    assert_eq!(v2.gates, std::array::from_fn(expected_gate));
    assert_eq!(v2.gate_count, 3);
    let v3 = loader::load_pe32_linked_v3_at(&image(&[3, 2, 1, 0]), BASE, GATE, 6)
        .unwrap()
        .into_parts()
        .1;
    assert_eq!(v3.gates, std::array::from_fn(expected_gate));
    assert_eq!(v3.gate_count, 4);
    let v4 = loader::load_pe32_linked_v4_at(&image(&[4, 3, 2, 1, 0]), BASE, GATE, 6)
        .unwrap()
        .into_parts()
        .1;
    assert_eq!(v4.gates, std::array::from_fn(expected_gate));
    assert_eq!(v4.gate_count, 5);
    let v5 = loader::load_pe32_linked_v5_at(&image(&[5, 4, 3, 2, 1, 0]), BASE, GATE, 6)
        .unwrap()
        .into_parts()
        .1;
    assert_eq!(v5.gates, std::array::from_fn(expected_gate));
    assert_eq!(v5.gate_count, 6);
}

#[test]
fn v6_keeps_the_eight_import_slot_limit_and_deduplicates_gates() {
    let eight = image(&[6, 5, 4, 3, 2, 1, 0, 6]);
    let linked = loader::load_pe32_linked_v6_at(&eight, BASE, GATE, 6)
        .unwrap()
        .into_parts()
        .1;
    assert_eq!(linked.gate_count, 7);
    assert_eq!(linked.gates, std::array::from_fn(expected_gate));
    assert_eq!(
        loader::load_pe32_linked_v6_at(&image(&[6; 9]), BASE, GATE, 6).err(),
        Some(LoadError::Capacity)
    );
}

#[test]
fn failed_v6_input_keeps_the_staged_image_and_arena_for_a_valid_retry() {
    let bytes = image(&[6, 4]);
    let mut engine = EngineInstance::new(6, KEY).unwrap();
    engine.begin_image_input(bytes.len() as u32).unwrap();
    engine.append_image_input(0, &bytes[..17]).unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.load_pe32_linked_v6_input_at(BASE, GATE),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.arena(), before);
    engine.append_image_input(17, &bytes[17..]).unwrap();
    assert_eq!(
        engine.load_pe32_linked_v5_input_at(BASE, GATE),
        Err(HostError::Loader(LoadError::Unsupported))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(
        engine.load_pe32_linked_v6_input_at(BASE, BASE + 0x2000),
        Err(HostError::Loader(LoadError::Malformed))
    );
    assert_eq!(engine.arena(), before);
    let linked = engine.load_pe32_linked_v6_input_at(BASE, GATE).unwrap();
    assert_eq!(linked.gate_count, 2);
    assert_eq!(&linked.gates[..2], &[expected_gate(4), expected_gate(6)]);
    assert_eq!(engine.arena(), before);
    assert_eq!(
        engine.load_pe32_linked_v6_input_at(BASE, GATE),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.arena(), before);
}
