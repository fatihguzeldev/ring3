use ring3_engine::{
    abi::arena::{ARENA_SIZE, HELPER_OFFSET},
    cpu::dbt::RegistryError,
    memory::GuestAddress,
    process::{EngineInstance, HostError, StoreCompletion},
};

const KEY: u64 = 0x1234_5678_9abc_def0;
const CODE: u32 = 0x8000;
const KEEP: u32 = 0xa000;

fn upload(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
}

fn describe(engine: &mut EngineInstance, address: u32) {
    engine.arena_mut().unwrap()[140..148]
        .copy_from_slice(&[address.to_le_bytes(), 3_u32.to_le_bytes()].concat());
}

fn fixture() -> (EngineInstance, u64, u64) {
    let mut engine = EngineInstance::new(6, KEY).unwrap();
    for address in [CODE, KEEP] {
        engine.map(address, 1, 7).unwrap();
        upload(&mut engine, address, &[0x90, 0xeb, 0]);
    }
    describe(&mut engine, CODE);
    engine.compile(1).unwrap();
    describe(&mut engine, CODE);
    let resident = engine.compile_resident(1).unwrap().get();
    describe(&mut engine, KEEP);
    let keep = engine.compile_resident(1).unwrap().get();
    engine.arena_mut().unwrap()[..100].fill(0xa5);
    (engine, resident, keep)
}

fn store(
    engine: &mut EngineInstance,
    resident: bool,
    id: u64,
    address: u32,
    value: u32,
) -> Result<StoreCompletion, HostError> {
    if resident {
        engine.store_resident16(KEY, id, address, value)
    } else {
        engine.store16(address, value)
    }
}

fn ram(engine: &EngineInstance, address: u32, length: usize) -> Vec<u8> {
    let mut result = vec![0; length];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut result)
        .unwrap();
    result
}

fn helper_only(engine: &EngineInstance, before: &[u8], fields: [u32; 6]) {
    let mut expected = before.to_vec();
    let output = &mut expected[HELPER_OFFSET..HELPER_OFFSET + 40];
    output[..16].copy_from_slice(&[82, 51, 77, 72, 4, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0]);
    for (index, field) in fields.into_iter().enumerate() {
        output[16 + index * 4..20 + index * 4].copy_from_slice(&field.to_le_bytes());
    }
    assert_eq!(engine.arena().len(), ARENA_SIZE);
    assert_eq!(engine.arena(), expected);
}

fn current(engine: &EngineInstance, resident: u64, keep: u64) {
    engine.guard(KEY, engine.generation()).unwrap();
    engine.guard_resident(KEY, resident).unwrap();
    engine.guard_resident(KEY, keep).unwrap();
}

#[test]
fn both_owners_store_low16_little_endian_across_pages_and_at_the_last_word() {
    for resident in [false, true] {
        for start in [0x1fff, u32::MAX - 1] {
            let (mut engine, id, keep) = fixture();
            let first = start & !0xfff;
            engine
                .map(first, if start == 0x1fff { 2 } else { 1 }, 7)
                .unwrap();
            engine.map(0, 1, 3).unwrap();
            upload(&mut engine, 0, &[0x5a]);
            upload(
                &mut engine,
                start - 1,
                if start == 0x1fff {
                    &[0x11, 0x22, 0x33, 0x44]
                } else {
                    &[0x11, 0x22, 0x33]
                },
            );
            engine
                .protect(first, if start == 0x1fff { 2 } else { 1 }, 2)
                .unwrap();
            let before = engine.arena().to_vec();
            let pointer = engine.artifact_bytes().unwrap().as_ptr();
            assert_eq!(
                store(&mut engine, resident, id, start, 0xabcd_9081),
                Ok(StoreCompletion::Complete)
            );
            helper_only(&engine, &before, [0, 0, 0, 0, 0, 2]);
            current(&engine, id, keep);
            assert_eq!(engine.artifact_bytes().unwrap().as_ptr(), pointer);
            engine
                .protect(first, if start == 0x1fff { 2 } else { 1 }, 3)
                .unwrap();
            assert_eq!(
                ram(&engine, start - 1, if start == 0x1fff { 4 } else { 3 }),
                if start == 0x1fff {
                    vec![0x11, 0x81, 0x90, 0x44]
                } else {
                    vec![0x11, 0x81, 0x90]
                }
            );
            assert_eq!(ram(&engine, 0, 1), [0x5a]);
        }
    }
}

#[test]
fn second_page_faults_preserve_both_bytes_sentinels_versions_and_all_owners() {
    for resident in [false, true] {
        for permission in [false, true] {
            let (mut engine, id, keep) = fixture();
            engine.map(0x1000, 1, 7).unwrap();
            upload(&mut engine, 0x1ffe, &[0x11, 0x22]);
            if permission {
                engine.map(0x2000, 1, 7).unwrap();
                upload(&mut engine, 0x2000, &[0x33, 0x44]);
                engine.protect(0x2000, 1, 5).unwrap();
            }
            let first = engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(0x1ffe), 2)
                .unwrap();
            let second = permission.then(|| {
                engine
                    .memory()
                    .unwrap()
                    .snapshot_code(GuestAddress(0x2000), 2)
                    .unwrap()
            });
            let artifact = engine.artifact_bytes().unwrap().to_vec();
            let pointer = engine.artifact_bytes().unwrap().as_ptr();
            for _ in 0..2 {
                let before = engine.arena().to_vec();
                assert_eq!(
                    store(&mut engine, resident, id, 0x1fff, 0x9081),
                    Ok(StoreCompletion::Complete)
                );
                helper_only(
                    &engine,
                    &before,
                    [1, 0, if permission { 2 } else { 1 }, 0x2000, 2, 2],
                );
                assert_eq!(ram(&engine, 0x1ffe, 2), [0x11, 0x22]);
                assert!(engine.memory().unwrap().is_code_current(&first));
                if let Some(second) = &second {
                    assert_eq!(ram(&engine, 0x2000, 2), [0x33, 0x44]);
                    assert!(engine.memory().unwrap().is_code_current(second));
                }
                current(&engine, id, keep);
                assert_eq!(engine.artifact_bytes().unwrap(), artifact);
                assert_eq!(engine.artifact_bytes().unwrap().as_ptr(), pointer);
            }
            if permission {
                engine.protect(0x2000, 1, 3).unwrap();
            } else {
                engine.map(0x2000, 1, 3).unwrap();
                upload(&mut engine, 0x2000, &[0x33, 0x44]);
            }
            let before = engine.arena().to_vec();
            assert_eq!(
                store(&mut engine, resident, id, 0x1fff, 0x9081),
                Ok(StoreCompletion::Complete)
            );
            helper_only(&engine, &before, [0, 0, 0, 0, 0, 2]);
            assert_eq!(ram(&engine, 0x1ffe, 4), [0x11, 0x81, 0x90, 0x44]);
            current(&engine, id, keep);
        }
    }
}

#[test]
fn overflow_and_authority_failures_are_neutral_before_any_write() {
    let mut empty = EngineInstance::new(1, KEY).unwrap();
    let before = empty.arena().to_vec();
    assert_eq!(empty.store16(0, 0xffff), Err(HostError::InvalidArtifact));
    assert_eq!(empty.arena(), before);
    for resident in [false, true] {
        let (mut engine, id, keep) = fixture();
        engine.map(0xffff_f000, 1, 7).unwrap();
        engine.map(0, 1, 7).unwrap();
        upload(&mut engine, u32::MAX - 1, &[0x11, 0x22]);
        upload(&mut engine, 0, &[0x33]);
        let snapshot = engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(u32::MAX - 1), 2)
            .unwrap();
        let before = engine.arena().to_vec();
        assert_eq!(
            store(&mut engine, resident, id, u32::MAX, 0xffff),
            Ok(StoreCompletion::Complete)
        );
        helper_only(&engine, &before, [1, 0, 3, u32::MAX, 2, 2]);
        assert_eq!(ram(&engine, u32::MAX - 1, 2), [0x11, 0x22]);
        assert_eq!(ram(&engine, 0, 1), [0x33]);
        assert!(engine.memory().unwrap().is_code_current(&snapshot));
        current(&engine, id, keep);
        for (key, invalid_id, expected) in [
            (KEY ^ (1 << 32), id, HostError::InvalidArtifact),
            (
                KEY,
                id ^ (1 << 32),
                HostError::Resident(RegistryError::InvalidUnit),
            ),
        ] {
            let before = engine.arena().to_vec();
            assert_eq!(
                engine.store_resident16(key, invalid_id, 0, 0xffff),
                Err(expected)
            );
            assert_eq!(engine.arena(), before);
            assert_eq!(ram(&engine, 0, 1), [0x33]);
        }
        engine.write8(CODE, 0x90).unwrap();
        let before = engine.arena().to_vec();
        assert_eq!(
            store(&mut engine, resident, id, 0, 0xffff),
            Err(if resident {
                HostError::Resident(RegistryError::CodeInvalidated)
            } else {
                HostError::CodeInvalidated
            })
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(ram(&engine, 0, 1), [0x33]);
        engine.close();
        let before = engine.arena().to_vec();
        assert_eq!(engine.store16(0, 0xffff), Err(HostError::Closed));
        assert_eq!(
            engine.store_resident16(0, 0, 0, 0xffff),
            Err(HostError::Closed)
        );
        assert_eq!(engine.arena(), before);
    }
}

#[test]
fn changed_and_same_word_code_stores_invalidate_only_the_affected_pages() {
    for resident in [false, true] {
        for value in [0xeb90, 0x9090] {
            let (mut engine, id, keep) = fixture();
            let before = engine.arena().to_vec();
            assert_eq!(
                store(&mut engine, resident, id, CODE, value),
                Ok(StoreCompletion::CodeInvalidated)
            );
            helper_only(&engine, &before, [0, 0, 0, 0, 0, 2]);
            assert_eq!(ram(&engine, CODE, 3), [value as u8, (value >> 8) as u8, 0]);
            assert_eq!(
                engine.guard(KEY, engine.generation()),
                Err(HostError::CodeInvalidated)
            );
            assert_eq!(
                engine.guard_resident(KEY, id),
                Err(HostError::Resident(RegistryError::CodeInvalidated))
            );
            engine.guard_resident(KEY, keep).unwrap();
        }
        let (mut engine, id, keep) = fixture();
        let before = engine.arena().to_vec();
        assert_eq!(
            store(&mut engine, resident, id, KEEP + 0x100, 0),
            Ok(StoreCompletion::Complete)
        );
        helper_only(&engine, &before, [0, 0, 0, 0, 0, 2]);
        engine.guard(KEY, engine.generation()).unwrap();
        engine.guard_resident(KEY, id).unwrap();
        assert_eq!(
            engine.guard_resident(KEY, keep),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
    }
}
