use ring3_engine::cpu::dbt::{
    BlockSpec, CompileError, CompileLimits, CompiledRegion, InstructionError, RegionMetadata,
    RegistryError, RegistryLimits, RegistryUsage, ResidentRegistry, UnitId, compile_region,
};
use ring3_engine::cpu::{UnsupportedFeature, x86::decode::DecodeError};
use ring3_engine::memory::{AddressSpace, GuestAddress, PageRange, Permissions};

const A: u32 = 0x1000;
const B: u32 = 0x2000;
const BAD: u32 = 0x3000;
const A_CODE: [u8; 6] = [0x40, 0xe9, 0xfa, 0x0f, 0, 0];
const B_CODE: [u8; 7] = [0x49, 0x0f, 0x85, 0xf9, 0xef, 0xff, 0xff];

fn range(pc: u32) -> PageRange {
    PageRange::new(GuestAddress(pc), 1).unwrap()
}

fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(3).unwrap();
    for (pc, bytes) in [(A, A_CODE.as_slice()), (B, B_CODE.as_slice())] {
        memory.map_zeroed(range(pc), Permissions::ALL).unwrap();
        memory.write(GuestAddress(pc), bytes).unwrap();
    }
    memory.map_zeroed(range(BAD), Permissions::ALL).unwrap();
    memory.write(GuestAddress(BAD), &[0x0f, 0x06]).unwrap();
    memory
}

fn spec(pc: u32, bytes: usize) -> BlockSpec {
    BlockSpec {
        entry: GuestAddress(pc),
        byte_length: bytes as u32,
    }
}

struct SavedUnit {
    id: UnitId,
    pointer: usize,
    bytes_pointer: *const u8,
    bytes: Vec<u8>,
    metadata: RegionMetadata,
}

fn save(registry: &ResidentRegistry, memory: &AddressSpace, ids: &[UnitId]) -> Vec<SavedUnit> {
    ids.iter()
        .map(|&id| {
            let unit = registry.get(memory, id).unwrap();
            let bytes = unit.wasm_bytes(memory).unwrap();
            SavedUnit {
                id,
                pointer: unit as *const CompiledRegion as usize,
                bytes_pointer: bytes.as_ptr(),
                bytes: bytes.to_vec(),
                metadata: unit.metadata(),
            }
        })
        .collect()
}

fn preserved(
    registry: &ResidentRegistry,
    memory: &AddressSpace,
    saved: &[SavedUnit],
    usage: RegistryUsage,
) {
    assert_eq!(registry.usage(), usage);
    for expected in saved {
        let unit = registry.get(memory, expected.id).unwrap();
        assert_eq!(unit as *const CompiledRegion as usize, expected.pointer);
        assert_eq!(unit.metadata(), expected.metadata);
        let bytes = unit.wasm_bytes(memory).unwrap();
        assert_eq!(bytes.as_ptr(), expected.bytes_pointer);
        assert_eq!(bytes, expected.bytes);
    }
}

fn rejected(
    registry: &mut ResidentRegistry,
    memory: &AddressSpace,
    ids: &[UnitId],
    error: RegistryError,
    specs: &[BlockSpec],
    limits: CompileLimits,
) {
    let saved = save(registry, memory, ids);
    let usage = registry.usage();
    assert_eq!(registry.compile(memory, specs, limits), Err(error));
    preserved(registry, memory, &saved, usage);
}

fn instruction_error(pc: u32, cause: InstructionError) -> RegistryError {
    RegistryError::Compile(CompileError::Instruction {
        pc: GuestAddress(pc),
        cause,
    })
}

#[test]
fn two_units_keep_independent_ids_bytes_and_interior_lookup() {
    let memory = memory();
    let expected_a =
        compile_region(&memory, &[spec(A, A_CODE.len())], CompileLimits::default()).unwrap();
    let expected_b =
        compile_region(&memory, &[spec(B, B_CODE.len())], CompileLimits::default()).unwrap();
    let mut registry = ResidentRegistry::new(&memory, RegistryLimits::default()).unwrap();
    assert_eq!(
        registry.usage(),
        RegistryUsage {
            units: 0,
            wasm_bytes: 0
        }
    );
    let snapshots = [A, B].map(|pc| memory.snapshot_code(GuestAddress(pc), 1).unwrap());
    let a = registry
        .compile(&memory, &[spec(A, A_CODE.len())], CompileLimits::default())
        .unwrap();
    let a_pointer = registry.get(&memory, a).unwrap() as *const _ as usize;
    let a_bytes_pointer = registry
        .get(&memory, a)
        .unwrap()
        .wasm_bytes(&memory)
        .unwrap()
        .as_ptr();
    let b = registry
        .compile(&memory, &[spec(B, B_CODE.len())], CompileLimits::default())
        .unwrap();
    assert!(a.get() > 0);
    assert!(b.get() > a.get());
    assert_ne!(a, b);
    assert_eq!(
        registry.get(&memory, a).unwrap() as *const _ as usize,
        a_pointer
    );
    assert_eq!(
        registry
            .get(&memory, a)
            .unwrap()
            .wasm_bytes(&memory)
            .unwrap()
            .as_ptr(),
        a_bytes_pointer
    );
    assert_eq!(
        registry
            .get(&memory, a)
            .unwrap()
            .wasm_bytes(&memory)
            .unwrap(),
        expected_a.wasm_bytes(&memory).unwrap()
    );
    assert_eq!(
        registry
            .get(&memory, b)
            .unwrap()
            .wasm_bytes(&memory)
            .unwrap(),
        expected_b.wasm_bytes(&memory).unwrap()
    );
    assert_eq!(
        registry.get(&memory, a).unwrap().metadata(),
        expected_a.metadata()
    );
    assert_eq!(
        registry.get(&memory, b).unwrap().metadata(),
        expected_b.metadata()
    );
    assert_eq!(registry.lookup(&memory, GuestAddress(A)), Ok(a));
    assert_eq!(registry.lookup(&memory, GuestAddress(A + 1)), Ok(a));
    assert_eq!(registry.lookup(&memory, GuestAddress(B)), Ok(b));
    assert_eq!(registry.lookup(&memory, GuestAddress(B + 1)), Ok(b));
    for pc in [A + 2, A + 5, A + 6, B + 2, B + 6, B + 7, BAD] {
        assert_eq!(
            registry.lookup(&memory, GuestAddress(pc)),
            Err(RegistryError::NotFound {
                pc: GuestAddress(pc)
            })
        );
    }
    assert_eq!(
        registry.usage(),
        RegistryUsage {
            units: 2,
            wasm_bytes: expected_a.wasm_bytes(&memory).unwrap().len()
                + expected_b.wasm_bytes(&memory).unwrap().len()
        }
    );
    assert!(
        snapshots
            .iter()
            .all(|snapshot| memory.is_code_current(snapshot))
    );
    let mut bytes = [0; 7];
    memory
        .fetch(GuestAddress(A), &mut bytes[..A_CODE.len()])
        .unwrap();
    assert_eq!(&bytes[..A_CODE.len()], A_CODE);
    memory.fetch(GuestAddress(B), &mut bytes).unwrap();
    assert_eq!(bytes, B_CODE);
}

#[test]
fn registry_limits_accept_inclusive_boundaries_and_reject_outside_them() {
    let memory = memory();
    for limits in [
        RegistryLimits {
            units: 0,
            wasm_bytes: 1,
        },
        RegistryLimits {
            units: 9,
            wasm_bytes: 1,
        },
        RegistryLimits {
            units: usize::MAX,
            wasm_bytes: 1,
        },
        RegistryLimits {
            units: 1,
            wasm_bytes: 0,
        },
        RegistryLimits {
            units: 1,
            wasm_bytes: 524_289,
        },
        RegistryLimits {
            units: 1,
            wasm_bytes: usize::MAX,
        },
    ] {
        assert_eq!(
            ResidentRegistry::new(&memory, limits).err(),
            Some(RegistryError::InvalidLimits)
        );
    }
    let default = RegistryLimits::default();
    assert_eq!((default.units, default.wasm_bytes), (8, 524_288));
    for limits in [
        RegistryLimits {
            units: 1,
            wasm_bytes: 1,
        },
        default,
    ] {
        assert_eq!(
            ResidentRegistry::new(&memory, limits).unwrap().usage(),
            RegistryUsage {
                units: 0,
                wasm_bytes: 0
            }
        );
    }
}

#[test]
fn owner_identity_precedes_capacity_membership_and_pc_errors() {
    let memory = memory();
    let other = self::memory();
    let mut registry = ResidentRegistry::new(
        &memory,
        RegistryLimits {
            units: 1,
            wasm_bytes: 524_288,
        },
    )
    .unwrap();
    let a = registry
        .compile(&memory, &[spec(A, A_CODE.len())], CompileLimits::default())
        .unwrap();
    let mut foreign = ResidentRegistry::new(&memory, RegistryLimits::default()).unwrap();
    let foreign_id = foreign
        .compile(&memory, &[spec(B, B_CODE.len())], CompileLimits::default())
        .unwrap();
    assert_ne!(a, foreign_id);
    let saved = save(&registry, &memory, &[a]);
    let usage = registry.usage();
    assert_eq!(
        registry.get(&memory, foreign_id).err(),
        Some(RegistryError::InvalidUnit)
    );
    assert_eq!(
        foreign.get(&memory, a).err(),
        Some(RegistryError::InvalidUnit)
    );
    assert_eq!(
        registry.lookup(&memory, GuestAddress(B)),
        Err(RegistryError::NotFound {
            pc: GuestAddress(B)
        })
    );
    assert_eq!(
        registry.get(&other, foreign_id).err(),
        Some(RegistryError::WrongAddressSpace)
    );
    assert_eq!(
        registry.get(&other, a).err(),
        Some(RegistryError::WrongAddressSpace)
    );
    assert_eq!(
        registry.lookup(&other, GuestAddress(BAD)),
        Err(RegistryError::WrongAddressSpace)
    );
    assert_eq!(
        registry.compile(
            &other,
            &[],
            CompileLimits {
                instructions: 0,
                ..CompileLimits::default()
            }
        ),
        Err(RegistryError::WrongAddressSpace)
    );
    preserved(&registry, &memory, &saved, usage);
    assert_eq!(registry.lookup(&memory, GuestAddress(A + 1)), Ok(a));
    assert_eq!(foreign.lookup(&memory, GuestAddress(B + 1)), Ok(foreign_id));
}

#[test]
fn stale_units_keep_storage_and_fresh_same_pc_replacements_win_lookup() {
    for mutation in 0..3 {
        let mut memory = memory();
        let mut registry = ResidentRegistry::new(
            &memory,
            RegistryLimits {
                units: 3,
                wasm_bytes: 524_288,
            },
        )
        .unwrap();
        let old = registry
            .compile(&memory, &[spec(A, A_CODE.len())], CompileLimits::default())
            .unwrap();
        let b = registry
            .compile(&memory, &[spec(B, B_CODE.len())], CompileLimits::default())
            .unwrap();
        let saved_b = save(&registry, &memory, &[b]);
        let old_bytes = registry
            .get(&memory, old)
            .unwrap()
            .wasm_bytes(&memory)
            .unwrap()
            .to_vec();
        let before = registry.usage();
        match mutation {
            0 => memory.write(GuestAddress(A), &A_CODE).unwrap(),
            1 => memory.protect(range(A), Permissions::ALL).unwrap(),
            2 => {
                memory.unmap(range(A)).unwrap();
                memory.map_zeroed(range(A), Permissions::ALL).unwrap();
                memory.write(GuestAddress(A), &A_CODE).unwrap();
            }
            _ => unreachable!(),
        }
        assert_eq!(
            registry.get(&memory, old).err(),
            Some(RegistryError::CodeInvalidated)
        );
        assert_eq!(
            registry.lookup(&memory, GuestAddress(A + 1)),
            Err(RegistryError::CodeInvalidated)
        );
        assert_eq!(
            registry.lookup(&memory, GuestAddress(A + 2)),
            Err(RegistryError::NotFound {
                pc: GuestAddress(A + 2)
            })
        );
        preserved(&registry, &memory, &saved_b, before);
        let fresh = registry
            .compile(&memory, &[spec(A, A_CODE.len())], CompileLimits::default())
            .unwrap();
        assert!(fresh.get() > old.get());
        assert_eq!(registry.lookup(&memory, GuestAddress(A)), Ok(fresh));
        assert_eq!(registry.lookup(&memory, GuestAddress(A + 1)), Ok(fresh));
        assert_eq!(
            registry.get(&memory, old).err(),
            Some(RegistryError::CodeInvalidated)
        );
        assert_eq!(
            registry
                .get(&memory, fresh)
                .unwrap()
                .wasm_bytes(&memory)
                .unwrap(),
            old_bytes
        );
        let after = RegistryUsage {
            units: 3,
            wasm_bytes: before.wasm_bytes + old_bytes.len(),
        };
        preserved(&registry, &memory, &saved_b, after);
        rejected(
            &mut registry,
            &memory,
            &[b, fresh],
            RegistryError::UnitCapacity,
            &[spec(BAD, 2)],
            CompileLimits::default(),
        );
        assert_eq!(
            registry.get(&memory, old).err(),
            Some(RegistryError::CodeInvalidated)
        );
    }
}

#[test]
fn a_changed_page_invalidates_every_start_of_a_multi_block_unit() {
    let mut memory = memory();
    let b_snapshot = memory.snapshot_code(GuestAddress(B), B_CODE.len()).unwrap();
    let mut registry = ResidentRegistry::new(&memory, RegistryLimits::default()).unwrap();
    let whole = registry
        .compile(
            &memory,
            &[spec(A, A_CODE.len()), spec(B, B_CODE.len())],
            CompileLimits::default(),
        )
        .unwrap();
    let before = registry.usage();
    assert_eq!(
        registry
            .get(&memory, whole)
            .unwrap()
            .metadata()
            .instructions,
        4
    );
    memory.write(GuestAddress(A), &A_CODE).unwrap();
    assert!(memory.is_code_current(&b_snapshot));
    for pc in [A, A + 1, B, B + 1] {
        assert_eq!(
            registry.lookup(&memory, GuestAddress(pc)),
            Err(RegistryError::CodeInvalidated)
        );
    }
    let fresh_b = registry
        .compile(&memory, &[spec(B, B_CODE.len())], CompileLimits::default())
        .unwrap();
    assert_eq!(registry.lookup(&memory, GuestAddress(B + 1)), Ok(fresh_b));
    assert_eq!(
        registry.lookup(&memory, GuestAddress(A)),
        Err(RegistryError::CodeInvalidated)
    );
    assert_eq!(
        registry.lookup(&memory, GuestAddress(BAD)),
        Err(RegistryError::NotFound {
            pc: GuestAddress(BAD)
        })
    );
    assert_eq!(
        registry.get(&memory, whole).err(),
        Some(RegistryError::CodeInvalidated)
    );
    assert_eq!(
        registry.usage(),
        RegistryUsage {
            units: 2,
            wasm_bytes: before.wasm_bytes
                + registry
                    .get(&memory, fresh_b)
                    .unwrap()
                    .wasm_bytes(&memory)
                    .unwrap()
                    .len()
        }
    );
}

#[test]
fn eight_units_are_retained_and_unit_capacity_wins_before_compilation() {
    let mut memory = AddressSpace::new(1).unwrap();
    memory.map_zeroed(range(A), Permissions::ALL).unwrap();
    for index in 0..8 {
        memory
            .write(GuestAddress(A + index * 16), &[0x90, 0xeb, 0])
            .unwrap();
    }
    let mut registry = ResidentRegistry::new(&memory, RegistryLimits::default()).unwrap();
    let mut ids = Vec::new();
    for index in 0..8 {
        let id = registry
            .compile(
                &memory,
                &[spec(A + index * 16, 3)],
                CompileLimits::default(),
            )
            .unwrap();
        assert!(id.get() > 0);
        assert!(ids.iter().all(|&prior| prior != id));
        ids.push(id);
    }
    assert_eq!(registry.usage().units, 8);
    rejected(
        &mut registry,
        &memory,
        &ids,
        RegistryError::UnitCapacity,
        &[],
        CompileLimits {
            instructions: 0,
            ..CompileLimits::default()
        },
    );
    rejected(
        &mut registry,
        &memory,
        &ids,
        RegistryError::UnitCapacity,
        &[spec(BAD, 2)],
        CompileLimits::default(),
    );
    memory.write(GuestAddress(A), &[0x90, 0xeb, 0]).unwrap();
    let usage = registry.usage();
    assert_eq!(
        registry.compile(&memory, &[spec(A, 3)], CompileLimits::default()),
        Err(RegistryError::UnitCapacity)
    );
    assert_eq!(registry.usage(), usage);
    assert_eq!(
        registry.get(&memory, ids[0]).err(),
        Some(RegistryError::CodeInvalidated)
    );
    assert_eq!(
        registry.lookup(&memory, GuestAddress(A)),
        Err(RegistryError::CodeInvalidated)
    );
}

#[test]
fn exact_emitted_byte_caps_and_compiler_error_priority_preserve_units() {
    let mut memory = memory();
    let a_bytes = compile_region(&memory, &[spec(A, A_CODE.len())], CompileLimits::default())
        .unwrap()
        .wasm_bytes(&memory)
        .unwrap()
        .len();
    let b_bytes = compile_region(&memory, &[spec(B, B_CODE.len())], CompileLimits::default())
        .unwrap()
        .wasm_bytes(&memory)
        .unwrap()
        .len();
    let mut registry = ResidentRegistry::new(
        &memory,
        RegistryLimits {
            units: 3,
            wasm_bytes: a_bytes,
        },
    )
    .unwrap();
    let a = registry
        .compile(&memory, &[spec(A, A_CODE.len())], CompileLimits::default())
        .unwrap();
    rejected(
        &mut registry,
        &memory,
        &[a],
        RegistryError::ByteCapacity,
        &[spec(A, A_CODE.len())],
        CompileLimits::default(),
    );
    rejected(
        &mut registry,
        &memory,
        &[a],
        RegistryError::Compile(CompileError::InvalidLimits),
        &[spec(A, A_CODE.len())],
        CompileLimits {
            wasm_bytes: 0,
            ..CompileLimits::default()
        },
    );
    rejected(
        &mut registry,
        &memory,
        &[a],
        RegistryError::Compile(CompileError::WasmLimit),
        &[spec(A, A_CODE.len())],
        CompileLimits {
            wasm_bytes: 1,
            ..CompileLimits::default()
        },
    );
    rejected(
        &mut registry,
        &memory,
        &[a],
        RegistryError::Compile(CompileError::InvalidBlocks),
        &[],
        CompileLimits::default(),
    );
    for (bytes, cause) in [
        (
            &[0x0f, 0x06][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Privileged)),
        ),
        (
            &[0xc0, 0x20, 1][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0x01, 0x03, 0xeb, 0][..],
            InstructionError::BackendUnsupported,
        ),
    ] {
        memory.write(GuestAddress(BAD), bytes).unwrap();
        rejected(
            &mut registry,
            &memory,
            &[a],
            instruction_error(BAD, cause),
            &[spec(BAD, bytes.len())],
            CompileLimits::default(),
        );
    }
    let mut one_short = ResidentRegistry::new(
        &memory,
        RegistryLimits {
            units: 2,
            wasm_bytes: a_bytes - 1,
        },
    )
    .unwrap();
    rejected(
        &mut one_short,
        &memory,
        &[],
        RegistryError::ByteCapacity,
        &[spec(A, A_CODE.len())],
        CompileLimits::default(),
    );
    let mut aggregate_short = ResidentRegistry::new(
        &memory,
        RegistryLimits {
            units: 2,
            wasm_bytes: a_bytes + b_bytes - 1,
        },
    )
    .unwrap();
    let old = aggregate_short
        .compile(&memory, &[spec(A, A_CODE.len())], CompileLimits::default())
        .unwrap();
    rejected(
        &mut aggregate_short,
        &memory,
        &[old],
        RegistryError::ByteCapacity,
        &[spec(B, B_CODE.len())],
        CompileLimits::default(),
    );
    let mut exact = ResidentRegistry::new(
        &memory,
        RegistryLimits {
            units: 2,
            wasm_bytes: a_bytes + b_bytes,
        },
    )
    .unwrap();
    exact
        .compile(&memory, &[spec(A, A_CODE.len())], CompileLimits::default())
        .unwrap();
    exact
        .compile(&memory, &[spec(B, B_CODE.len())], CompileLimits::default())
        .unwrap();
    assert_eq!(
        exact.usage(),
        RegistryUsage {
            units: 2,
            wasm_bytes: a_bytes + b_bytes
        }
    );
}

#[test]
fn instruction_start_collisions_reject_but_distinct_overlapping_decodings_are_valid() {
    let mut memory = memory();
    memory
        .write(GuestAddress(A), &[0xb8, 0x90, 0x90, 0xeb, 0, 0xeb, 0])
        .unwrap();
    let mut registry = ResidentRegistry::new(&memory, RegistryLimits::default()).unwrap();
    let original = registry
        .compile(&memory, &[spec(A, 7)], CompileLimits::default())
        .unwrap();
    let overlapping = registry
        .compile(&memory, &[spec(A + 1, 4)], CompileLimits::default())
        .unwrap();
    assert_eq!(registry.lookup(&memory, GuestAddress(A)), Ok(original));
    assert_eq!(registry.lookup(&memory, GuestAddress(A + 5)), Ok(original));
    for pc in [A + 1, A + 2, A + 3] {
        assert_eq!(registry.lookup(&memory, GuestAddress(pc)), Ok(overlapping));
    }
    assert_eq!(
        registry.lookup(&memory, GuestAddress(A + 4)),
        Err(RegistryError::NotFound {
            pc: GuestAddress(A + 4)
        })
    );
    rejected(
        &mut registry,
        &memory,
        &[original, overlapping],
        RegistryError::InstructionOverlap {
            pc: GuestAddress(A + 5),
        },
        &[spec(A + 5, 2)],
        CompileLimits::default(),
    );
    rejected(
        &mut registry,
        &memory,
        &[original, overlapping],
        RegistryError::InstructionOverlap {
            pc: GuestAddress(A),
        },
        &[spec(A, 7)],
        CompileLimits::default(),
    );
    rejected(
        &mut registry,
        &memory,
        &[original, overlapping],
        RegistryError::Compile(CompileError::InvalidBlocks),
        &[spec(A, 7), spec(A + 1, 4)],
        CompileLimits::default(),
    );
}
