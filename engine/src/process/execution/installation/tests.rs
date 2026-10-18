use crate::{
    abi::arena::{CANCEL_OFFSET, TRANSFER_OFFSET},
    cpu::dbt::RegistryError,
    memory::GuestAddress,
    process::{CallError, EngineInstance, HostError, ResidentInstallation},
};

const KEY: u64 = 97;
const PCS: [u32; 4] = [0x1000, 0x1020, 0x1040, 0x1060];

fn compile(engine: &mut EngineInstance, pc: u32) -> u64 {
    let transfer = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    transfer[..4].copy_from_slice(&pc.to_le_bytes());
    transfer[4..].copy_from_slice(&3u32.to_le_bytes());
    engine.compile_resident(1).unwrap().get()
}

fn fixture() -> (EngineInstance, [u64; 4]) {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    for pc in PCS {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(pc), &[0x90, 0xeb, 0xfd])
            .unwrap();
    }
    let ids = PCS.map(|pc| compile(&mut engine, pc));
    (engine, ids)
}

#[test]
fn installation_lookup_accepts_out_of_order_acknowledgements_and_rejects_failed_publications() {
    let (mut engine, ids) = fixture();
    for (index, slot) in [(2, 0), (0, 7), (1, 3)] {
        engine
            .acknowledge_resident_installation(KEY, ids[index], slot)
            .unwrap();
    }
    let expected = vec![
        ResidentInstallation {
            unit_id: ids[0],
            slot: 7,
        },
        ResidentInstallation {
            unit_id: ids[1],
            slot: 3,
        },
        ResidentInstallation {
            unit_id: ids[2],
            slot: 0,
        },
    ];
    assert_eq!(engine.resident_installations_by_id, expected);
    for (pc, installed) in PCS.into_iter().zip(expected.iter()) {
        assert_eq!(engine.lookup_installed_resident(KEY, pc), Ok(*installed));
    }
    let slots = engine.resident_installations.clone();
    assert_eq!(
        engine.acknowledge_resident_installation(KEY, ids[0], 7),
        Ok(expected[0])
    );
    for (id, slot) in [(ids[0], 2), (ids[3], 3), (ids[3], 8)] {
        assert_eq!(
            engine.acknowledge_resident_installation(KEY, id, slot),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.resident_installations, slots);
        assert_eq!(engine.resident_installations_by_id, expected);
    }
    engine.arena_mut().unwrap()[CANCEL_OFFSET..CANCEL_OFFSET + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        engine.acknowledge_resident_installation(KEY, ids[3], 2),
        Err(HostError::Call(CallError::Cancelled))
    );
    assert_eq!(engine.resident_installations, slots);
    assert_eq!(engine.resident_installations_by_id, expected);
}

#[test]
fn discarded_and_stale_installations_release_their_index_before_slot_reuse() {
    let (mut engine, ids) = fixture();
    engine
        .acknowledge_resident_installation(KEY, ids[1], 0)
        .unwrap();
    engine
        .acknowledge_resident_installation(KEY, ids[0], 7)
        .unwrap();
    engine.discard_installed_resident(KEY, ids[1], 0).unwrap();
    let replacement = compile(&mut engine, PCS[1]);
    engine
        .acknowledge_resident_installation(KEY, replacement, 0)
        .unwrap();
    assert_eq!(
        engine.lookup_installed_resident(KEY, PCS[1]),
        Ok(ResidentInstallation {
            unit_id: replacement,
            slot: 0,
        })
    );
    assert_eq!(
        engine.guard_resident(KEY, ids[1]),
        Err(HostError::Resident(RegistryError::InvalidUnit))
    );
    assert_eq!(
        engine.resident_installations_by_id,
        [
            ResidentInstallation {
                unit_id: ids[0],
                slot: 7,
            },
            ResidentInstallation {
                unit_id: replacement,
                slot: 0,
            },
        ]
    );
    engine.write8(PCS[0], 0x90).unwrap();
    assert_eq!(
        engine.lookup_installed_resident(KEY, PCS[1]),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    engine.retire_stale_resident(KEY, replacement).unwrap();
    engine.retire_stale_resident(KEY, ids[0]).unwrap();
    assert!(engine.resident_installations_by_id.is_empty());
    assert!(engine.resident_installations.iter().all(Option::is_none));
}

#[test]
fn reverse_candidate_never_authorizes_a_missing_or_different_canonical_slot() {
    let (mut engine, ids) = fixture();
    engine
        .acknowledge_resident_installation(KEY, ids[0], 3)
        .unwrap();
    for canonical in [
        None,
        Some(ResidentInstallation {
            unit_id: ids[1],
            slot: 3,
        }),
        Some(ResidentInstallation {
            unit_id: ids[0],
            slot: 7,
        }),
    ] {
        engine.resident_installations[3] = canonical;
        assert_eq!(
            engine.lookup_installed_resident(KEY ^ 1, PCS[0]),
            Err(HostError::InvalidArtifact)
        );
        assert_eq!(
            engine.lookup_installed_resident(KEY, PCS[0]),
            Err(HostError::Resident(RegistryError::NotFound {
                pc: GuestAddress(PCS[0]),
            }))
        );
    }
    engine.close();
    assert!(engine.resident_installations.is_empty());
    assert!(engine.resident_installations_by_id.is_empty());
    assert_eq!(
        engine.lookup_installed_resident(KEY, PCS[0]),
        Err(HostError::Closed)
    );
}
