#[allow(dead_code)]
#[path = "support/pe32.rs"]
mod pe;

#[allow(dead_code)]
#[path = "support/pe32_relocation.rs"]
mod reloc;

use ring3_engine::{
    cpu::dbt::GateSpec,
    loader::{self, LoadError},
    memory::{Access, AddressSpace, GuestAddress},
    process::{EngineInstance, HostError},
};

const ALLOWED: [u16; 4] = [0, 0x0100, 0x8000, 0x8100];
const ACTUAL: u32 = 0x0050_0000;
const GATE: u32 = 0x7000_0000;
const KEY: u64 = 0xfedc_ba98_7654_3210;

fn linked_image() -> Vec<u8> {
    let mut bytes = reloc::full_image();
    bytes[pe::DATA_RAW + 0x100..pe::DATA_RAW + 0x240].fill(0);
    for (index, rva, size) in [(1, 0x3100, 40), (12, 0x3180, 8)] {
        pe::put32(&mut bytes, pe::OPTIONAL + 96 + index * 8, rva);
        pe::put32(&mut bytes, pe::OPTIONAL + 100 + index * 8, size);
    }
    for (index, value) in [0x3140, 0, 0, 0x31c0, 0x3180].into_iter().enumerate() {
        pe::put32(&mut bytes, pe::DATA_RAW + 0x100 + index * 4, value);
    }
    for offset in [0x140, 0x180] {
        pe::put32(&mut bytes, pe::DATA_RAW + offset, 0x3200);
    }
    bytes[pe::DATA_RAW + 0x1c0..pe::DATA_RAW + 0x1cd].copy_from_slice(b"KERNEL32.dll\0");
    bytes[pe::DATA_RAW + 0x202..pe::DATA_RAW + 0x214].copy_from_slice(b"GetCurrentProcess\0");
    bytes
}

fn read(memory: &AddressSpace, address: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    memory.read(GuestAddress(address), &mut bytes).unwrap();
    bytes
}

fn permissions(memory: &AddressSpace, base: u32) {
    for (rva, expected) in [
        (0, [true, false, false]),
        (0x1000, [true, false, true]),
        (0x3000, [true, true, false]),
    ] {
        for (access, allowed) in [Access::Read, Access::Write, Access::Execute]
            .into_iter()
            .zip(expected)
        {
            assert_eq!(
                memory.resolve(GuestAddress(base + rva), access).is_ok(),
                allowed
            );
        }
    }
}

fn relocated_words(memory: &AddressSpace) {
    for ((rva, _), value) in
        reloc::WORDS
            .into_iter()
            .zip([0x0050_3000_u32, 0x000f_fffc, 0x0010_3000, 0x0010_0002])
    {
        assert_eq!(read(memory, ACTUAL + rva, 4), value.to_le_bytes());
    }
}

#[test]
fn allowed_dll_characteristics_load_across_loader_profiles() {
    for flags in ALLOWED {
        let mut fixed = pe::image();
        pe::put16(&mut fixed, pe::OPTIONAL + 70, flags);
        let original = fixed.clone();
        let (memory, image) = loader::load_pe32(&fixed, 4).unwrap().into_parts();
        assert_eq!(
            (
                image.image_base,
                image.image_size,
                image.entry_point,
                image.mapped_pages
            ),
            (pe::BASE, 0x7000, pe::BASE + 0x1000, 4)
        );
        assert_eq!(read(&memory, pe::BASE, 512), original[..512]);
        permissions(&memory, pe::BASE);
        assert_eq!(fixed, original);

        let mut relocated = reloc::full_image();
        pe::put16(&mut relocated, pe::OPTIONAL + 70, flags);
        let original = relocated.clone();
        let (memory, image) = loader::load_pe32_at(&relocated, ACTUAL, 6)
            .unwrap()
            .into_parts();
        assert_eq!(
            (
                image.image_base,
                image.image_size,
                image.entry_point,
                image.mapped_pages
            ),
            (ACTUAL, 0x9000, ACTUAL + 0x1000, 6)
        );
        assert_eq!(read(&memory, ACTUAL, 512), original[..512]);
        relocated_words(&memory);
        permissions(&memory, ACTUAL);
        assert_eq!(relocated, original);

        let mut linked = linked_image();
        pe::put16(&mut linked, pe::OPTIONAL + 70, flags);
        let original = linked.clone();
        let (memory, metadata) = loader::load_pe32_linked_v8_at(&linked, ACTUAL, GATE, 7)
            .unwrap()
            .into_parts();
        assert_eq!(
            (
                metadata.image.image_base,
                metadata.image.image_size,
                metadata.image.entry_point,
                metadata.image.mapped_pages
            ),
            (ACTUAL, 0x9000, ACTUAL + 0x1000, 7)
        );
        assert_eq!(metadata.gate_base, GATE);
        assert_eq!(metadata.gate_count, 1);
        assert_eq!(
            metadata.gates[0],
            GateSpec {
                entry: GuestAddress(GATE + 128),
                id: 0x10009
            }
        );
        assert_eq!(
            read(&memory, ACTUAL + 0x3180, 8),
            [GATE + 128, 0]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>()
        );
        assert_eq!(read(&memory, GATE + 128, 2), [0x0f, 0x0b]);
        assert_eq!(read(&memory, ACTUAL, 512), original[..512]);
        relocated_words(&memory);
        permissions(&memory, ACTUAL);
        assert!(
            memory
                .resolve(GuestAddress(GATE + 128), Access::Execute)
                .is_ok()
        );
        assert!(
            memory
                .resolve(GuestAddress(GATE + 128), Access::Write)
                .is_err()
        );
        assert_eq!(linked, original);
    }
}

fn unsupported_flags() -> impl Iterator<Item = u16> {
    (0..16)
        .filter(|bit| *bit != 8 && *bit != 15)
        .flat_map(|bit| ALLOWED.map(|allowed| allowed | (1_u16 << bit)))
}

#[test]
fn every_other_dll_characteristics_bit_remains_unsupported_in_all_profiles() {
    let flags: Vec<_> = unsupported_flags().collect();
    assert_eq!(flags.len(), 56);
    for flags in flags {
        let mut fixed = pe::image();
        pe::put16(&mut fixed, pe::OPTIONAL + 70, flags);
        let original = fixed.clone();
        assert_eq!(
            loader::load_pe32(&fixed, 4).err(),
            Some(LoadError::Unsupported),
            "fixed {flags:#06x}"
        );
        assert_eq!(fixed, original);

        let mut relocated = reloc::full_image();
        pe::put16(&mut relocated, pe::OPTIONAL + 70, flags);
        let original = relocated.clone();
        assert_eq!(
            loader::load_pe32_at(&relocated, ACTUAL, 6).err(),
            Some(LoadError::Unsupported),
            "relocated {flags:#06x}"
        );
        assert_eq!(relocated, original);

        let mut linked = linked_image();
        pe::put16(&mut linked, pe::OPTIONAL + 70, flags);
        let original = linked.clone();
        assert_eq!(
            loader::load_pe32_linked_v8_at(&linked, ACTUAL, GATE, 7).err(),
            Some(LoadError::Unsupported),
            "linked {flags:#06x}"
        );
        assert_eq!(linked, original);
    }
}

fn process_load(engine: &mut EngineInstance, profile: u8, bytes: &[u8]) -> Result<(), HostError> {
    match profile {
        0 => engine.load_pe32(bytes).map(|_| ()),
        1 => engine.load_pe32_at(bytes, ACTUAL).map(|_| ()),
        2 => engine
            .load_pe32_linked_v8_at(bytes, ACTUAL, GATE)
            .map(|_| ()),
        _ => unreachable!(),
    }
}

fn sentinel(engine: &mut EngineInstance) {
    for (index, byte) in engine.arena_mut().unwrap().iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(31).wrapping_add(7);
    }
}

fn observation(engine: &EngineInstance) -> (Vec<u8>, usize, Vec<u8>, u32, u32) {
    (
        engine.arena().to_vec(),
        engine.arena_address(),
        engine.dispatcher_bytes(KEY).unwrap().to_vec(),
        engine.memory().unwrap().capacity_pages(),
        engine.memory().unwrap().mapped_pages(),
    )
}

#[test]
fn unsupported_flags_preserve_pristine_publication_and_allow_valid_retry() {
    for (profile, mut bytes, expected_pages, base) in [
        (0, pe::image(), 4, pe::BASE),
        (1, reloc::full_image(), 6, ACTUAL),
        (2, linked_image(), 7, ACTUAL),
    ] {
        let mut engine = EngineInstance::new(7, KEY).unwrap();
        sentinel(&mut engine);
        let before = observation(&engine);
        assert_eq!(before.0.len(), 4364);
        for flags in unsupported_flags() {
            pe::put16(&mut bytes, pe::OPTIONAL + 70, flags);
            let input = bytes.clone();
            assert_eq!(
                process_load(&mut engine, profile, &bytes),
                Err(HostError::Loader(LoadError::Unsupported)),
                "profile {profile}, flags {flags:#06x}"
            );
            assert_eq!(observation(&engine), before);
            assert_eq!(engine.key(), KEY);
            assert_eq!(engine.generation(), 0);
            assert!(engine.is_open());
            assert_eq!(bytes, input);
        }
        pe::put16(&mut bytes, pe::OPTIONAL + 70, 0x8100);
        let input = bytes.clone();
        process_load(&mut engine, profile, &bytes).unwrap();
        assert_eq!(engine.arena(), before.0);
        assert_eq!(engine.arena_address(), before.1);
        assert_eq!(engine.dispatcher_bytes(KEY).unwrap(), before.2);
        assert_eq!(engine.memory().unwrap().capacity_pages(), before.3);
        assert_eq!(engine.memory().unwrap().mapped_pages(), expected_pages);
        assert_eq!(engine.key(), KEY);
        assert_eq!(engine.generation(), 0);
        assert_eq!(read(engine.memory().unwrap(), base, 512), input[..512]);
        assert_eq!(bytes, input);
        assert_eq!(
            process_load(&mut engine, profile, &bytes),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.arena(), before.0);
    }
}

#[test]
fn nonpristine_flag_admission_preserves_both_compiled_owners() {
    for resident in [false, true] {
        let mut engine = EngineInstance::new(7, KEY).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        engine.arena_mut().unwrap()[140..145].copy_from_slice(&[0xb8, 42, 0, 0, 0]);
        engine.upload(0x1000, 5).unwrap();
        pe::put32(engine.arena_mut().unwrap(), 140, 0x1000);
        pe::put32(engine.arena_mut().unwrap(), 144, 5);
        let id = if resident {
            engine.compile_resident(1).unwrap().get()
        } else {
            u64::from(engine.compile(1).unwrap())
        };
        sentinel(&mut engine);
        let snapshot = engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(0x1000), 5)
            .unwrap();
        let before = observation(&engine);
        let ram = read(engine.memory().unwrap(), 0x1000, 4096);
        let generation = engine.generation();
        let module = if resident {
            engine.resident_bytes(id).unwrap()
        } else {
            engine.artifact_bytes().unwrap()
        };
        let module_bytes = module.to_vec();
        let module_pointer = module.as_ptr();
        for (profile, mut bytes) in [
            (0, pe::image()),
            (1, reloc::full_image()),
            (2, linked_image()),
        ] {
            for flags in [0x8100, 0x8140] {
                pe::put16(&mut bytes, pe::OPTIONAL + 70, flags);
                let input = bytes.clone();
                assert_eq!(
                    process_load(&mut engine, profile, &bytes),
                    Err(HostError::InvalidRequest)
                );
                assert_eq!(observation(&engine), before);
                assert_eq!(read(engine.memory().unwrap(), 0x1000, 4096), ram);
                assert!(engine.memory().unwrap().is_code_current(&snapshot));
                assert_eq!(engine.generation(), generation);
                assert_eq!(engine.key(), KEY);
                let module = if resident {
                    engine.resident_bytes(id).unwrap()
                } else {
                    engine.artifact_bytes().unwrap()
                };
                assert_eq!(module, module_bytes);
                assert_eq!(module.as_ptr(), module_pointer);
                if resident {
                    assert_eq!(engine.guard_resident(KEY, id), Ok(()));
                } else {
                    assert_eq!(engine.guard(KEY, id as u32), Ok(()));
                }
                assert_eq!(bytes, input);
            }
        }
    }
}
