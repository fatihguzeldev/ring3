use super::*;
use crate::memory::{PageRange, Permissions};

const KEY: u64 = 0x1020_3040_5060_7080;
const A: GuestAddress = GuestAddress(0x1000);
const B: GuestAddress = GuestAddress(0x2000);
const GATE: GuestAddress = GuestAddress(0x1100);

fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(PageRange::new(A, 2).unwrap(), Permissions::ALL)
        .unwrap();
    for pc in [A, B] {
        memory.write(pc, &[0x40, 0xeb, 0, 0x0f, 0x0b]).unwrap();
    }
    memory.write(GATE, &[0x0f, 0x0b]).unwrap();
    memory
}

fn registry(memory: &AddressSpace) -> ResidentRegistry {
    ResidentRegistry::new(memory, RegistryLimits::default()).unwrap()
}

fn compile(
    registry: &mut ResidentRegistry,
    memory: &AddressSpace,
    entries: &[GuestAddress],
    gates: &[GateSpec],
    limits: CompileLimits,
    counter: &AtomicU64,
) -> Result<UnitId, RegistryError> {
    registry.compile_entries_bound_with_counter(memory, entries, gates, limits, KEY, counter)
}

fn bytes(registry: &ResidentRegistry, memory: &AddressSpace, id: UnitId) -> Vec<u8> {
    registry
        .get(memory, id)
        .unwrap()
        .wasm_bytes(memory)
        .unwrap()
        .to_vec()
}

fn retained(
    registry: &ResidentRegistry,
    memory: &AddressSpace,
    id: UnitId,
    usage: RegistryUsage,
    expected: &[u8],
    pointer: *const u8,
) {
    assert_eq!(registry.usage(), usage);
    let actual = registry
        .get(memory, id)
        .unwrap()
        .wasm_bytes(memory)
        .unwrap();
    assert_eq!(actual, expected);
    assert_eq!(actual.as_ptr(), pointer);
}

#[test]
fn resident_cold_families_match_literal_extents_and_existing_full_u64_bound_emission() {
    let cases: &[(&[u8], bool, usize)] = &[
        (&[0xb8, 0x78, 0x56, 0x34, 0x12], false, 2),
        (&[0x8d, 0x40, 7], false, 2),
        (&[0x0f, 0xb6, 0xc1], false, 2),
        (&[0x40, 0x83, 0xe8, 1, 0x85, 0xc0], false, 4),
        (&[0xa1, 0, 0, 0xad, 0xde], false, 2),
        (&[0x0f, 0xb6, 0x05, 0, 0, 0xad, 0xde], false, 2),
        (&[0x03, 0x05, 0, 0, 0xad, 0xde], false, 2),
        (&[0x85, 0x05, 0, 0, 0xad, 0xde], false, 2),
        (&[0xa3, 0, 0, 0xad, 0xde], false, 2),
        (&[0xff, 0x05, 0, 0, 0xad, 0xde], false, 2),
        (&[0x83, 0x05, 0, 0, 0xad, 0xde, 1], false, 2),
        (&[0x50, 0x59], false, 3),
        (&[0xff, 0x35, 0, 0, 0xad, 0xde], false, 2),
        (&[0x8f, 0x05, 0, 0, 0xad, 0xde], false, 2),
        (&[0xe9, 0, 0, 0, 0], true, 1),
        (&[0x75, 0], true, 1),
        (&[0xff, 0xe0], true, 1),
        (&[0xff, 0x25, 0, 0, 0xad, 0xde], true, 1),
        (&[0xe8, 0, 0, 0, 0], true, 1),
        (&[0xff, 0xd0], true, 1),
        (&[0xff, 0x15, 0, 0, 0xad, 0xde], true, 1),
        (&[0xc3], true, 1),
        (&[0xc2, 8, 0], true, 1),
    ];
    for &(prefix, terminal, instructions) in cases {
        let mut memory = memory();
        let mut code = prefix.to_vec();
        if !terminal {
            code.extend([0xeb, 0]);
        }
        let byte_length = code.len() as u32;
        code.extend([0x0f, 0x0b]);
        memory.write(A, &code).unwrap();
        let mut cold = registry(&memory);
        let mut explicit = registry(&memory);
        let cold_counter = AtomicU64::new(0x1234_5678_9abc_def0);
        let explicit_counter = AtomicU64::new(0x1234_5678_9abc_def0);
        let cold_id = compile(
            &mut cold,
            &memory,
            &[A],
            &[],
            CompileLimits::default(),
            &cold_counter,
        )
        .unwrap();
        let explicit_id = explicit
            .compile_bound_with_counter(
                &memory,
                &[BlockSpec {
                    entry: A,
                    byte_length,
                }],
                &[],
                CompileLimits::default(),
                KEY,
                &explicit_counter,
            )
            .unwrap();
        assert_eq!(cold_id, explicit_id);
        let cold_region = cold.get(&memory, cold_id).unwrap();
        let explicit_region = explicit.get(&memory, explicit_id).unwrap();
        assert_eq!(
            (
                cold_region.metadata().blocks,
                cold_region.metadata().instructions
            ),
            (1, instructions),
            "{prefix:02x?}"
        );
        assert_eq!(cold_region.metadata(), explicit_region.metadata());
        assert_eq!(
            cold_region.wasm_bytes(&memory).unwrap(),
            explicit_region.wasm_bytes(&memory).unwrap(),
            "{prefix:02x?}"
        );
        assert_eq!(cold_counter.load(Ordering::Relaxed), 0x1234_5678_9abc_def1);
        assert_eq!(cold.usage(), explicit.usage());
    }
}

#[test]
fn unordered_cold_gates_match_existing_snapshot_and_executable_address_ownership() {
    let mut memory = memory();
    let second_gate = GuestAddress(0x1200);
    memory.write(second_gate, &[0x0f, 0x0b]).unwrap();
    let entries = [second_gate, B, GATE, A];
    let gates = [
        GateSpec {
            entry: GATE,
            id: 17,
        },
        GateSpec {
            entry: second_gate,
            id: 18,
        },
    ];
    let specs = [
        BlockSpec {
            entry: second_gate,
            byte_length: 2,
        },
        BlockSpec {
            entry: B,
            byte_length: 3,
        },
        BlockSpec {
            entry: GATE,
            byte_length: 2,
        },
        BlockSpec {
            entry: A,
            byte_length: 3,
        },
    ];
    let mut cold = registry(&memory);
    let mut explicit = registry(&memory);
    let cold_id = compile(
        &mut cold,
        &memory,
        &entries,
        &gates,
        CompileLimits::default(),
        &AtomicU64::new(0x1_0000_0001),
    )
    .unwrap();
    let explicit_id = explicit
        .compile_bound_with_counter(
            &memory,
            &specs,
            &gates,
            CompileLimits::default(),
            KEY,
            &AtomicU64::new(0x1_0000_0001),
        )
        .unwrap();
    assert_eq!(cold_id.get(), 0x1_0000_0001);
    assert_eq!(
        bytes(&cold, &memory, cold_id),
        bytes(&explicit, &memory, explicit_id)
    );
    let region = cold.get(&memory, cold_id).unwrap();
    assert_eq!(
        (region.metadata().blocks, region.metadata().instructions),
        (4, 6)
    );
    assert_eq!(
        region
            .executable_addresses()
            .map(|pc| pc.0)
            .collect::<Vec<_>>(),
        [0x1200, 0x2000, 0x2001, 0x1100, 0x1000, 0x1001]
    );
    assert!(region.matches_gate(GATE.0, 17));
    assert!(region.matches_gate(second_gate.0, 18));
    assert!(!region.matches_gate(GATE.0, 18));
    memory.write(B, &[0x40]).unwrap();
    assert_eq!(
        cold.get(&memory, cold_id).unwrap_err(),
        RegistryError::CodeInvalidated
    );
}

#[test]
fn preparation_failures_leave_local_counter_and_retained_usage_unchanged() {
    let mut memory = memory();
    let unsupported = GuestAddress(0x2100);
    memory.write(unsupported, &[0x0f, 0x06]).unwrap();
    let mut registry = registry(&memory);
    let counter = AtomicU64::new(41);
    let first = compile(
        &mut registry,
        &memory,
        &[A],
        &[],
        CompileLimits::default(),
        &counter,
    )
    .unwrap();
    assert_eq!(first.get(), 41);
    let original = bytes(&registry, &memory, first);
    let pointer = registry
        .get(&memory, first)
        .unwrap()
        .wasm_bytes(&memory)
        .unwrap()
        .as_ptr();
    let usage = registry.usage();
    for (entries, gates, limits, expected) in [
        (
            vec![],
            vec![],
            CompileLimits::default(),
            RegistryError::Compile(CompileError::InvalidBlocks),
        ),
        (
            vec![B, B],
            vec![],
            CompileLimits::default(),
            RegistryError::Compile(CompileError::InvalidBlocks),
        ),
        (
            vec![B],
            vec![GateSpec {
                entry: GATE,
                id: 17,
            }],
            CompileLimits::default(),
            RegistryError::Compile(CompileError::InvalidGates),
        ),
        (
            vec![B],
            vec![],
            CompileLimits {
                blocks: 0,
                ..CompileLimits::default()
            },
            RegistryError::Compile(CompileError::InvalidLimits),
        ),
        (
            vec![B],
            vec![],
            CompileLimits {
                instructions: 1,
                ..CompileLimits::default()
            },
            RegistryError::Compile(CompileError::InstructionLimit),
        ),
        (
            vec![GATE],
            vec![GateSpec { entry: GATE, id: 0 }],
            CompileLimits::default(),
            RegistryError::Compile(CompileError::InvalidGates),
        ),
    ] {
        assert_eq!(
            compile(&mut registry, &memory, &entries, &gates, limits, &counter),
            Err(expected)
        );
        assert_eq!(counter.load(Ordering::Relaxed), 42);
        retained(&registry, &memory, first, usage, &original, pointer);
    }
    assert!(matches!(
        compile(
            &mut registry,
            &memory,
            &[unsupported],
            &[],
            CompileLimits::default(),
            &counter
        ),
        Err(RegistryError::Compile(CompileError::Instruction { .. }))
    ));
    assert_eq!(counter.load(Ordering::Relaxed), 42);
    retained(&registry, &memory, first, usage, &original, pointer);
    assert_eq!(
        compile(
            &mut registry,
            &memory,
            &[B],
            &[],
            CompileLimits::default(),
            &counter
        )
        .unwrap()
        .get(),
        42
    );
}

#[test]
fn emission_overlap_and_byte_admission_failures_leave_permanent_local_id_holes() {
    let memory = memory();
    let mut registry = registry(&memory);
    let counter = AtomicU64::new(0x1_0000_0001);
    let first = compile(
        &mut registry,
        &memory,
        &[A],
        &[],
        CompileLimits::default(),
        &counter,
    )
    .unwrap();
    let original = bytes(&registry, &memory, first);
    let pointer = registry
        .get(&memory, first)
        .unwrap()
        .wasm_bytes(&memory)
        .unwrap()
        .as_ptr();
    let usage = registry.usage();
    assert_eq!(
        compile(
            &mut registry,
            &memory,
            &[B],
            &[],
            CompileLimits {
                wasm_bytes: 1,
                ..CompileLimits::default()
            },
            &counter
        ),
        Err(RegistryError::Compile(CompileError::WasmLimit))
    );
    assert_eq!(counter.load(Ordering::Relaxed), 0x1_0000_0003);
    retained(&registry, &memory, first, usage, &original, pointer);
    assert_eq!(
        compile(
            &mut registry,
            &memory,
            &[A],
            &[],
            CompileLimits::default(),
            &counter
        ),
        Err(RegistryError::InstructionOverlap { pc: A })
    );
    assert_eq!(counter.load(Ordering::Relaxed), 0x1_0000_0004);
    retained(&registry, &memory, first, usage, &original, pointer);
    for hole in [0x1_0000_0002, 0x1_0000_0003] {
        assert_eq!(
            registry.get_raw(&memory, hole).unwrap_err(),
            RegistryError::InvalidUnit
        );
    }
    assert_eq!(
        compile(
            &mut registry,
            &memory,
            &[B],
            &[],
            CompileLimits::default(),
            &counter
        )
        .unwrap()
        .get(),
        0x1_0000_0004
    );

    let mut tight = ResidentRegistry::new(
        &memory,
        RegistryLimits {
            units: 8,
            wasm_bytes: original.len(),
        },
    )
    .unwrap();
    let counter = AtomicU64::new(0x1_0000_0001);
    let first = compile(
        &mut tight,
        &memory,
        &[A],
        &[],
        CompileLimits::default(),
        &counter,
    )
    .unwrap();
    let original = bytes(&tight, &memory, first);
    let pointer = tight
        .get(&memory, first)
        .unwrap()
        .wasm_bytes(&memory)
        .unwrap()
        .as_ptr();
    let usage = tight.usage();
    assert_eq!(
        compile(
            &mut tight,
            &memory,
            &[B],
            &[],
            CompileLimits::default(),
            &counter
        ),
        Err(RegistryError::ByteCapacity)
    );
    assert_eq!(counter.load(Ordering::Relaxed), 0x1_0000_0003);
    assert_eq!(
        tight.get_raw(&memory, 0x1_0000_0002).unwrap_err(),
        RegistryError::InvalidUnit
    );
    retained(&tight, &memory, first, usage, &original, pointer);
}

#[test]
fn memory_domain_and_full_unit_capacity_precede_decode_and_never_reserve_id() {
    let memory = memory();
    let foreign = AddressSpace::new(1).unwrap();
    let mut registry = ResidentRegistry::new(
        &memory,
        RegistryLimits {
            units: 1,
            ..RegistryLimits::default()
        },
    )
    .unwrap();
    let counter = AtomicU64::new(73);
    let first = compile(
        &mut registry,
        &memory,
        &[A],
        &[],
        CompileLimits::default(),
        &counter,
    )
    .unwrap();
    let original = bytes(&registry, &memory, first);
    let pointer = registry
        .get(&memory, first)
        .unwrap()
        .wasm_bytes(&memory)
        .unwrap()
        .as_ptr();
    let usage = registry.usage();
    assert_eq!(
        compile(
            &mut registry,
            &foreign,
            &[],
            &[],
            CompileLimits {
                blocks: 0,
                ..CompileLimits::default()
            },
            &counter
        ),
        Err(RegistryError::WrongAddressSpace)
    );
    assert_eq!(
        compile(
            &mut registry,
            &memory,
            &[],
            &[],
            CompileLimits {
                blocks: 0,
                ..CompileLimits::default()
            },
            &counter
        ),
        Err(RegistryError::UnitCapacity)
    );
    assert_eq!(counter.load(Ordering::Relaxed), 74);
    retained(&registry, &memory, first, usage, &original, pointer);
}

#[test]
fn local_identity_exhaustion_precedes_emission_and_cannot_wrap_or_publish() {
    let memory = memory();
    for initial in [0, u64::MAX] {
        let mut registry = registry(&memory);
        let counter = AtomicU64::new(initial);
        assert_eq!(
            compile(
                &mut registry,
                &memory,
                &[A],
                &[],
                CompileLimits {
                    wasm_bytes: 1,
                    ..CompileLimits::default()
                },
                &counter
            ),
            Err(RegistryError::IdentityExhausted)
        );
        assert_eq!(counter.load(Ordering::Relaxed), initial);
        assert_eq!(
            registry.usage(),
            RegistryUsage {
                units: 0,
                wasm_bytes: 0
            }
        );
    }
    let mut registry = registry(&memory);
    let counter = AtomicU64::new(u64::MAX - 1);
    let id = compile(
        &mut registry,
        &memory,
        &[A],
        &[],
        CompileLimits::default(),
        &counter,
    )
    .unwrap();
    assert_eq!(id.get(), u64::MAX - 1);
    assert_eq!(counter.load(Ordering::Relaxed), u64::MAX);
    let original = bytes(&registry, &memory, id);
    let pointer = registry
        .get(&memory, id)
        .unwrap()
        .wasm_bytes(&memory)
        .unwrap()
        .as_ptr();
    let usage = registry.usage();
    assert_eq!(
        compile(
            &mut registry,
            &memory,
            &[B],
            &[],
            CompileLimits::default(),
            &counter
        ),
        Err(RegistryError::IdentityExhausted)
    );
    assert_eq!(counter.load(Ordering::Relaxed), u64::MAX);
    retained(&registry, &memory, id, usage, &original, pointer);
}

#[test]
fn stale_code_admission_and_fresh_successor_preserve_existing_unit_accounting() {
    let mut memory = memory();
    let mut registry = registry(&memory);
    let counter = AtomicU64::new(11);
    let retained_id = compile(
        &mut registry,
        &memory,
        &[B],
        &[],
        CompileLimits::default(),
        &counter,
    )
    .unwrap();
    let retained_bytes = bytes(&registry, &memory, retained_id);
    let retained_pointer = registry
        .get(&memory, retained_id)
        .unwrap()
        .wasm_bytes(&memory)
        .unwrap()
        .as_ptr();
    let old_id = compile(
        &mut registry,
        &memory,
        &[A],
        &[],
        CompileLimits::default(),
        &counter,
    )
    .unwrap();
    let before = registry.usage();
    let prepared = compile_region(
        &memory,
        &[BlockSpec {
            entry: A,
            byte_length: 3,
        }],
        CompileLimits::default(),
    )
    .unwrap();
    memory.write(A, &[0x40]).unwrap();
    assert_eq!(
        registry.admit(&memory, &prepared),
        Err(RegistryError::CodeInvalidated)
    );
    retained(
        &registry,
        &memory,
        retained_id,
        before,
        &retained_bytes,
        retained_pointer,
    );
    assert_eq!(
        registry.get_raw(&memory, old_id.get()).unwrap_err(),
        RegistryError::CodeInvalidated
    );
    let new_id = compile(
        &mut registry,
        &memory,
        &[A],
        &[],
        CompileLimits::default(),
        &counter,
    )
    .unwrap();
    assert_eq!((old_id.get(), new_id.get()), (12, 13));
    assert_eq!(registry.usage().units, 3);
    assert_eq!(registry.lookup(&memory, A), Ok(new_id));
    assert_eq!(registry.lookup(&memory, B), Ok(retained_id));
    assert_eq!(bytes(&registry, &memory, retained_id), retained_bytes);
    assert_eq!(
        registry
            .get(&memory, retained_id)
            .unwrap()
            .wasm_bytes(&memory)
            .unwrap()
            .as_ptr(),
        retained_pointer
    );
}
