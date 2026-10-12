use ring3_engine::{
    abi::{
        arena::{TRANSFER_OFFSET, TRANSFER_SIZE},
        x86::{encode_exit_v3, encode_state},
    },
    cpu::{ExecutionExit, ExitReason, dbt::RegistryError, x86::State32},
    memory::GuestAddress,
    process::{CallError, EngineInstance, HostError, ResidentInstallation},
    windows::CallingConvention32,
};

const KEY: u64 = 0x1020_3040_5060_7080;
const A: u32 = 0x1000;
const B: u32 = 0x2000;
const GATE: u32 = 0x3000;
const WHOLE: u32 = 0x4000;
const CALLBACK_RETURN: u32 = 0x5000;
const CROSS: u32 = 0x5ffd;
const DATA: u32 = 0x8100;
const STACK: u32 = 0xa100;
const A_CODE: [u8; 8] = [0xb8, 1, 2, 3, 4, 0x90, 0xeb, 0];
const B_CODE: [u8; 3] = [0x90, 0xeb, 0];
const PAGES: [u32; 8] = [A, B, GATE, WHOLE, 0x5000, 0x6000, 0x8000, 0xa000];

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn compile(engine: &mut EngineInstance, specs: &[(u32, u32)], gates: &[(u32, u32)]) -> u64 {
    let transfer =
        &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE];
    transfer.fill(0xa5);
    for (index, &(pc, value)) in specs.iter().chain(gates).enumerate() {
        transfer[index * 8..index * 8 + 4].copy_from_slice(&pc.to_le_bytes());
        transfer[index * 8 + 4..index * 8 + 8].copy_from_slice(&value.to_le_bytes());
    }
    let before = engine.arena().to_vec();
    let id = engine
        .compile_resident_with_gates(specs.len() as u32, gates.len() as u32)
        .unwrap()
        .get();
    assert_eq!(engine.arena(), before);
    id
}

fn fixture(entry: u32) -> (EngineInstance, Vec<u64>) {
    let mut engine = EngineInstance::new(12, KEY).unwrap();
    for page in PAGES {
        engine.map(page, 1, 7).unwrap();
    }
    upload(&mut engine, entry, &A_CODE);
    upload(&mut engine, WHOLE, &B_CODE);
    for pc in (0..=5).map(|index| B + index * 0x10) {
        upload(&mut engine, pc, &B_CODE);
    }
    for pc in [GATE, CALLBACK_RETURN] {
        upload(&mut engine, pc, &[0x0f, 0x0b]);
    }
    let ids = vec![
        compile(&mut engine, &[(entry, 8), (WHOLE, 3)], &[]),
        compile(&mut engine, &[(B, 3)], &[]),
        compile(&mut engine, &[(GATE, 2)], &[(GATE, 17)]),
    ];
    for (address, value) in [
        (DATA - 4, 0x89abcdef),
        (DATA, 0x11223344),
        (DATA + 4, 0x55667788),
        (STACK - 4, 0xdeadbeef),
        (STACK, B),
        (STACK + 4, 0xaabbccdd),
    ] {
        engine.write32(address, value).unwrap();
    }
    (engine, ids)
}

fn pcs(entry: u32) -> Vec<u32> {
    let mut pcs = vec![
        entry,
        entry + 5,
        entry + 6,
        WHOLE,
        WHOLE + 1,
        B,
        B + 1,
        GATE,
    ];
    for index in 1..=5 {
        pcs.extend([B + index * 0x10, B + index * 0x10 + 1]);
    }
    pcs
}

fn not_found(pc: u32) -> HostError {
    HostError::Resident(RegistryError::NotFound {
        pc: GuestAddress(pc),
    })
}

fn installed(id: u64, slot: u32) -> ResidentInstallation {
    ResidentInstallation { unit_id: id, slot }
}

#[derive(Debug, PartialEq, Eq)]
struct UnitObservation {
    id: u64,
    bytes: Result<Vec<u8>, HostError>,
    pointer: Option<usize>,
    guard: Result<(), HostError>,
}

#[derive(Debug, PartialEq, Eq)]
struct Core {
    arena: Vec<u8>,
    arena_address: usize,
    generation: u32,
    legacy: Result<Vec<u8>, HostError>,
    units: Vec<UnitObservation>,
    logical: Vec<Result<u64, HostError>>,
    ram: Vec<Result<Vec<u8>, HostError>>,
}

fn core(engine: &EngineInstance, ids: &[u64], pcs: &[u32]) -> Core {
    Core {
        arena: engine.arena().to_vec(),
        arena_address: engine.arena_address(),
        generation: engine.generation(),
        legacy: engine.artifact_bytes().map(|bytes| bytes.to_vec()),
        units: ids
            .iter()
            .map(|&id| UnitObservation {
                id,
                bytes: engine.resident_bytes(id).map(|bytes| bytes.to_vec()),
                pointer: engine
                    .resident_bytes(id)
                    .ok()
                    .map(|bytes| bytes.as_ptr() as usize),
                guard: engine.guard_resident(KEY, id),
            })
            .collect(),
        logical: pcs
            .iter()
            .map(|&pc| engine.lookup_resident(pc).map(|id| id.get()))
            .collect(),
        ram: PAGES
            .iter()
            .map(|&pc| {
                let memory = engine.memory()?;
                let mut bytes = vec![0; 4096];
                memory
                    .read(GuestAddress(pc), &mut bytes)
                    .map_err(HostError::Memory)?;
                Ok(bytes)
            })
            .collect(),
    }
}

fn bindings(engine: &EngineInstance, pcs: &[u32]) -> Vec<Result<ResidentInstallation, HostError>> {
    pcs.iter()
        .map(|&pc| engine.lookup_installed_resident(KEY, pc))
        .collect()
}

fn preserved(
    engine: &mut EngineInstance,
    ids: &[u64],
    pcs: &[u32],
    action: impl FnOnce(&mut EngineInstance),
) {
    let before = core(engine, ids, pcs);
    let before_bindings = bindings(engine, pcs);
    action(engine);
    assert_eq!(core(engine, ids, pcs), before);
    assert_eq!(bindings(engine, pcs), before_bindings);
}

fn acknowledge(engine: &mut EngineInstance, ids: &[u64], pcs: &[u32], id: u64, slot: u32) {
    let before = core(engine, ids, pcs);
    assert_eq!(
        engine.acknowledge_resident_installation(KEY, id, slot),
        Ok(installed(id, slot))
    );
    assert_eq!(core(engine, ids, pcs), before);
}

#[test]
fn resident_installation_is_hidden_until_acknowledgement() {
    let (mut engine, ids) = fixture(A);
    let pcs = pcs(A);
    for &pc in &[A, A + 5, A + 6, WHOLE, B, GATE] {
        preserved(&mut engine, &ids, &pcs, |engine| {
            assert_eq!(
                engine.lookup_installed_resident(KEY, pc),
                Err(not_found(pc))
            );
            assert!(engine.lookup_resident(pc).is_ok());
        });
    }
    assert_eq!(engine.guard_resident(KEY, ids[0]), Ok(()));
    assert_eq!(engine.generation(), 0);
    acknowledge(&mut engine, &ids, &pcs, ids[0], 0);
    for pc in [A, A + 5, A + 6, WHOLE, WHOLE + 1] {
        preserved(&mut engine, &ids, &pcs, |engine| {
            assert_eq!(
                engine.lookup_installed_resident(KEY, pc),
                Ok(installed(ids[0], 0))
            );
        });
    }
    for pc in [0, u32::MAX, A + 1, A + 7, WHOLE + 2, A + 8, 0x9000] {
        preserved(&mut engine, &ids, &pcs, |engine| {
            assert_eq!(
                engine.lookup_installed_resident(KEY, pc),
                Err(not_found(pc))
            );
        });
    }
    acknowledge(&mut engine, &ids, &pcs, ids[1], 7);
    acknowledge(&mut engine, &ids, &pcs, ids[2], 4);
    preserved(&mut engine, &ids, &pcs, |engine| {
        assert_eq!(
            engine.acknowledge_resident_installation(KEY, ids[0], 0),
            Ok(installed(ids[0], 0))
        );
        assert_eq!(
            engine.acknowledge_resident_installation(KEY, ids[1], 7),
            Ok(installed(ids[1], 7))
        );
        assert_eq!(
            engine.lookup_installed_resident(KEY, B + 1),
            Ok(installed(ids[1], 7))
        );
        assert_eq!(
            engine.lookup_installed_resident(KEY, GATE),
            Ok(installed(ids[2], 4))
        );
    });
}

#[test]
fn identity_slots_collisions_and_cancel_priorities_preserve_all_state() {
    let (mut engine, mut ids) = fixture(A);
    for index in 1..=5 {
        ids.push(compile(&mut engine, &[(B + index * 0x10, 3)], &[]));
    }
    let pcs = pcs(A);
    let (_, foreign_ids) = fixture(A);
    for (key, id, slot, error) in [
        (KEY ^ (1 << 32), 0, u32::MAX, HostError::InvalidArtifact),
        (
            KEY,
            0,
            u32::MAX,
            HostError::Resident(RegistryError::InvalidUnit),
        ),
        (
            KEY,
            ids[0] ^ (1 << 32),
            0,
            HostError::Resident(RegistryError::InvalidUnit),
        ),
        (
            KEY,
            foreign_ids[0],
            0,
            HostError::Resident(RegistryError::InvalidUnit),
        ),
        (KEY, ids[0], 8, HostError::InvalidRequest),
        (KEY, ids[0], u32::MAX, HostError::InvalidRequest),
    ] {
        preserved(&mut engine, &ids, &pcs, |engine| {
            assert_eq!(
                engine.acknowledge_resident_installation(key, id, slot),
                Err(error)
            );
        });
    }
    preserved(&mut engine, &ids, &pcs, |engine| {
        assert_eq!(
            engine.lookup_installed_resident(KEY ^ (1 << 32), u32::MAX),
            Err(HostError::InvalidArtifact)
        );
    });
    acknowledge(&mut engine, &ids, &pcs, ids[0], 0);
    acknowledge(&mut engine, &ids, &pcs, ids[1], 7);
    engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    for (id, slot, error) in [
        (ids[0], 7, HostError::InvalidRequest),
        (ids[0], 1, HostError::InvalidRequest),
        (ids[2], 0, HostError::InvalidRequest),
        (ids[2], 8, HostError::InvalidRequest),
        (ids[0], 0, HostError::Call(CallError::Cancelled)),
        (ids[2], 4, HostError::Call(CallError::Cancelled)),
    ] {
        preserved(&mut engine, &ids, &pcs, |engine| {
            assert_eq!(
                engine.acknowledge_resident_installation(KEY, id, slot),
                Err(error)
            );
        });
    }
    engine.arena_mut().unwrap()[96..100].fill(0);
    for (index, slot) in [4, 1, 2, 3, 5, 6].into_iter().enumerate() {
        acknowledge(&mut engine, &ids, &pcs, ids[index + 2], slot);
    }
    for ((id, slot), pc) in ids
        .iter()
        .copied()
        .zip([0, 7, 4, 1, 2, 3, 5, 6])
        .zip([A, B, GATE, 0x2010, 0x2020, 0x2030, 0x2040, 0x2050])
    {
        preserved(&mut engine, &ids, &pcs, |engine| {
            assert_eq!(
                engine.acknowledge_resident_installation(KEY, id, slot),
                Ok(installed(id, slot))
            );
            assert_eq!(
                engine.lookup_installed_resident(KEY, pc),
                Ok(installed(id, slot))
            );
        });
    }
    upload(&mut engine, A, &A_CODE);
    preserved(&mut engine, &ids, &pcs, |engine| {
        assert_eq!(
            engine.acknowledge_resident_installation(KEY, ids[0], u32::MAX),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
        assert_eq!(
            engine.lookup_installed_resident(KEY, A),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
        assert_eq!(
            engine.acknowledge_resident_installation(KEY ^ 1, ids[0], 0),
            Err(HostError::InvalidArtifact)
        );
        assert_eq!(
            engine.lookup_installed_resident(KEY, B),
            Ok(installed(ids[1], 7))
        );
    });
}

#[test]
fn snapshot_changes_retain_old_slot_and_fresh_unit_requires_its_own_ack() {
    for mutation in [
        "same-byte",
        "protect",
        "remap",
        "whole-unit",
        "cross-lower",
        "cross-upper",
    ] {
        let entry = if mutation.starts_with("cross-") {
            CROSS
        } else {
            A
        };
        let (mut engine, mut ids) = fixture(entry);
        let pcs = pcs(entry);
        let old = ids[0];
        acknowledge(&mut engine, &ids, &pcs, old, 0);
        acknowledge(&mut engine, &ids, &pcs, ids[1], 7);
        engine.write32(DATA, 0x88776655).unwrap();
        engine.write32(STACK, 0x2001).unwrap();
        engine.arena_mut().unwrap()[140..252].fill(0xc3);
        preserved(&mut engine, &ids, &pcs, |engine| {
            assert_eq!(
                engine.lookup_installed_resident(KEY, entry),
                Ok(installed(old, 0))
            );
            assert_eq!(
                engine.acknowledge_resident_installation(KEY, old, 0),
                Ok(installed(old, 0))
            );
        });
        match mutation {
            "protect" => engine.protect(A, 1, 5).unwrap(),
            "remap" => {
                engine.unmap(A, 1).unwrap();
                engine.map(A, 1, 7).unwrap();
                upload(&mut engine, A, &A_CODE);
            }
            "whole-unit" => upload(&mut engine, WHOLE, &B_CODE),
            "cross-lower" => upload(&mut engine, CROSS, &A_CODE[..1]),
            "cross-upper" => upload(&mut engine, 0x6000, &A_CODE[3..4]),
            "same-byte" => upload(&mut engine, A, &A_CODE),
            _ => unreachable!(),
        }
        engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
        preserved(&mut engine, &ids, &pcs, |engine| {
            assert_eq!(
                engine.acknowledge_resident_installation(KEY, old, u32::MAX),
                Err(HostError::Resident(RegistryError::CodeInvalidated))
            );
            assert_eq!(
                engine.lookup_installed_resident(KEY, entry),
                Err(HostError::Resident(RegistryError::CodeInvalidated))
            );
            assert_eq!(
                engine.lookup_installed_resident(KEY, WHOLE),
                Err(HostError::Resident(RegistryError::CodeInvalidated))
            );
            assert_eq!(
                engine.lookup_installed_resident(KEY, B),
                Ok(installed(ids[1], 7))
            );
        });
        engine.arena_mut().unwrap()[96..100].fill(0);
        let fresh = compile(&mut engine, &[(entry, 8), (WHOLE, 3)], &[]);
        assert_ne!(fresh, old);
        ids.push(fresh);
        preserved(&mut engine, &ids, &pcs, |engine| {
            assert_eq!(engine.lookup_resident(entry).unwrap().get(), fresh);
            assert_eq!(
                engine.lookup_installed_resident(KEY, entry),
                Err(not_found(entry))
            );
            assert_eq!(
                engine.lookup_installed_resident(KEY, WHOLE),
                Err(not_found(WHOLE))
            );
            assert_eq!(
                engine.acknowledge_resident_installation(KEY, fresh, 0),
                Err(HostError::InvalidRequest)
            );
            assert_eq!(
                engine.acknowledge_resident_installation(KEY, old, 0),
                Err(HostError::Resident(RegistryError::CodeInvalidated))
            );
        });
        acknowledge(&mut engine, &ids, &pcs, fresh, 1);
        preserved(&mut engine, &ids, &pcs, |engine| {
            assert_eq!(
                engine.lookup_installed_resident(KEY, entry + 5),
                Ok(installed(fresh, 1))
            );
            assert_eq!(
                engine.lookup_installed_resident(KEY, WHOLE),
                Ok(installed(fresh, 1))
            );
            assert_eq!(
                engine.lookup_installed_resident(KEY, B),
                Ok(installed(ids[1], 7))
            );
        });
    }
}

fn install_legacy(engine: &mut EngineInstance) -> u32 {
    let transfer = &mut engine.arena_mut().unwrap()[140..];
    for (index, (pc, value)) in [
        (B, 3_u32),
        (GATE, 2),
        (CALLBACK_RETURN, 2),
        (GATE, 17),
        (CALLBACK_RETURN, 18),
    ]
    .into_iter()
    .enumerate()
    {
        transfer[index * 8..index * 8 + 4].copy_from_slice(&pc.to_le_bytes());
        transfer[index * 8 + 4..index * 8 + 8].copy_from_slice(&value.to_le_bytes());
    }
    engine.compile_with_gates(3, 2).unwrap()
}

fn typed_gate_stop(engine: &mut EngineInstance) {
    let arena = engine.arena_mut().unwrap();
    encode_state(
        &State32 {
            registers: [1, 2, 3, 4, STACK, 6, 7, 8],
            eip: GATE,
            eflags: 0xcd7,
        },
        &mut arena[..56],
    )
    .unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id: 17 },
        },
        &mut arena[56..96],
    )
    .unwrap();
    arena[96..100].fill(0);
}

#[test]
fn pending_callback_cancel_and_close_preserve_inspection_and_ack_priorities() {
    let (mut engine, ids) = fixture(A);
    let pcs = pcs(A);
    let generation = install_legacy(&mut engine);
    acknowledge(&mut engine, &ids, &pcs, ids[0], 0);
    acknowledge(&mut engine, &ids, &pcs, ids[2], 2);
    // typed native stop setup; actual translated handoff belongs to the wasm proof.
    typed_gate_stop(&mut engine);
    let frame = engine
        .capture_resident_call(KEY, ids[2], CallingConvention32::Cdecl, 0)
        .unwrap();
    preserved(&mut engine, &ids, &pcs, |engine| {
        assert_eq!(
            engine.lookup_installed_resident(KEY, GATE),
            Ok(installed(ids[2], 2))
        );
        assert_eq!(
            engine.lookup_installed_resident(KEY, A),
            Ok(installed(ids[0], 0))
        );
        assert_eq!(engine.lookup_installed_resident(KEY, B), Err(not_found(B)));
        assert_eq!(
            engine.acknowledge_resident_installation(KEY, ids[1], u32::MAX),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(
            engine.acknowledge_resident_installation(KEY, ids[0], 0),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(
            engine.acknowledge_resident_installation(KEY, 0, 0),
            Err(HostError::Resident(RegistryError::InvalidUnit))
        );
        assert_eq!(
            engine.acknowledge_resident_installation(KEY ^ 1, ids[1], 3),
            Err(HostError::InvalidArtifact)
        );
    });
    upload(&mut engine, A, &A_CODE);
    engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    engine.arena_mut().unwrap()[0] ^= 1;
    engine.arena_mut().unwrap()[56] ^= 1;
    preserved(&mut engine, &ids, &pcs, |engine| {
        assert_eq!(
            engine.acknowledge_resident_installation(KEY, ids[0], u32::MAX),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
        assert_eq!(
            engine.acknowledge_resident_installation(KEY, ids[1], 3),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(
            engine.lookup_installed_resident(KEY, GATE),
            Ok(installed(ids[2], 2))
        );
        assert_eq!(
            engine.lookup_installed_resident(KEY, A),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
    });
    engine.abandon_call(KEY, frame.token).unwrap();
    for (id, slot, error) in [
        (ids[1], 3, HostError::Call(CallError::Cancelled)),
        (ids[2], 2, HostError::Call(CallError::Cancelled)),
        (ids[2], 0, HostError::InvalidRequest),
        (ids[1], 8, HostError::InvalidRequest),
    ] {
        preserved(&mut engine, &ids, &pcs, |engine| {
            assert_eq!(
                engine.acknowledge_resident_installation(KEY, id, slot),
                Err(error)
            );
        });
    }
    engine.arena_mut().unwrap()[96..100].fill(0);
    acknowledge(&mut engine, &ids, &pcs, ids[1], 3);
    // explicitly typed legacy outer only establishes callback busy/inspection compatibility.
    typed_gate_stop(&mut engine);
    let outer = engine
        .capture_call(KEY, generation, CallingConvention32::Cdecl, 0)
        .unwrap();
    engine
        .begin_callback(KEY, generation, outer.token, B, CALLBACK_RETURN, 18, &[])
        .unwrap();
    engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    engine.arena_mut().unwrap()[0] ^= 1;
    preserved(&mut engine, &ids, &pcs, |engine| {
        assert_eq!(
            engine.lookup_installed_resident(KEY, B + 1),
            Ok(installed(ids[1], 3))
        );
        assert_eq!(
            engine.lookup_installed_resident(KEY, GATE),
            Ok(installed(ids[2], 2))
        );
        assert_eq!(
            engine.acknowledge_resident_installation(KEY, ids[1], u32::MAX),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(
            engine.acknowledge_resident_installation(KEY, ids[0], 0),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
        assert_eq!(
            engine.acknowledge_resident_installation(KEY, 0, 0),
            Err(HostError::Resident(RegistryError::InvalidUnit))
        );
    });
    let arena = engine.arena().to_vec();
    engine.close();
    assert_eq!(engine.arena(), arena);
    preserved(&mut engine, &ids, &pcs, |engine| {
        assert_eq!(
            engine.acknowledge_resident_installation(0, 0, u32::MAX),
            Err(HostError::Closed)
        );
        assert_eq!(
            engine.lookup_installed_resident(0, u32::MAX),
            Err(HostError::Closed)
        );
        assert_eq!(
            engine.lookup_installed_resident(KEY, GATE),
            Err(HostError::Closed)
        );
    });
}
