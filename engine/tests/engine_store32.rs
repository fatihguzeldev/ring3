use ring3_engine::memory::GuestAddress;
use ring3_engine::process::{EngineInstance, HostError, StoreCompletion};

const KEY: u64 = 0xfedc_ba98_7654_3210;

fn compile(engine: &mut EngineInstance, entry: u32) -> u32 {
    let descriptor = &mut engine.arena_mut().unwrap()[140..148];
    descriptor[..4].copy_from_slice(&entry.to_le_bytes());
    descriptor[4..].copy_from_slice(&1_u32.to_le_bytes());
    engine.compile(1).unwrap()
}

fn with_code(entry: u32) -> EngineInstance {
    let mut engine = EngineInstance::new(4, KEY).unwrap();
    engine.map(entry & !0xfff, 1, 7).unwrap();
    engine.write32(entry, 0x9090_9090).unwrap();
    compile(&mut engine, entry);
    engine
}

fn guest_word(engine: &EngineInstance, address: u32) -> u32 {
    let mut bytes = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    u32::from_le_bytes(bytes)
}

fn helper_record(fields: [u32; 6]) -> [u8; 40] {
    let mut bytes = [0; 40];
    bytes[..16].copy_from_slice(&[0x52, 0x33, 0x4d, 0x48, 1, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0]);
    for (index, field) in fields.into_iter().enumerate() {
        bytes[16 + index * 4..20 + index * 4].copy_from_slice(&field.to_le_bytes());
    }
    bytes
}

fn poison_helper(engine: &mut EngineInstance) {
    engine.arena_mut().unwrap()[100..140].fill(0xa5);
}

fn assert_only_helper_changed(engine: &EngineInstance, before: &[u8]) {
    assert_eq!(&engine.arena()[..100], &before[..100]);
    assert_eq!(&engine.arena()[140..], &before[140..]);
}

#[test]
fn unrelated_executable_page_store_completes_and_keeps_installed_code_current() {
    let mut engine = with_code(0x1000);
    engine.map(0x4000, 2, 7).unwrap();
    let generation = engine.generation();
    let artifact = engine.artifact_bytes().unwrap().to_vec();
    for address in [0x4001, 0x4ffe] {
        poison_helper(&mut engine);
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.store32(address, 0x4433_2211),
            Ok(StoreCompletion::Complete)
        );
        assert_eq!(guest_word(&engine, address), 0x4433_2211);
        assert_eq!(&engine.arena()[100..140], &helper_record([0; 6]));
        assert_only_helper_changed(&engine, &before);
        assert_eq!(engine.generation(), generation);
        assert_eq!(engine.artifact_bytes().unwrap(), artifact);
        engine.guard(KEY, generation).unwrap();
    }
}

#[test]
fn covered_page_store_commits_then_reports_invalidation_even_for_identical_bytes() {
    for (address, value) in [(0x1000, 0x9090_9090), (0x1008, 0x1122_3344)] {
        let mut engine = with_code(0x1000);
        engine.map(0x4000, 1, 3).unwrap();
        let generation = engine.generation();
        poison_helper(&mut engine);
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.store32(address, value),
            Ok(StoreCompletion::CodeInvalidated)
        );
        assert_eq!(guest_word(&engine, address), value);
        assert_eq!(&engine.arena()[100..140], &helper_record([0; 6]));
        assert_only_helper_changed(&engine, &before);
        assert_eq!(engine.generation(), generation);
        assert_eq!(
            engine.guard(KEY, generation),
            Err(HostError::CodeInvalidated)
        );
        assert_eq!(engine.artifact_bytes(), Err(HostError::CodeInvalidated));
        let stale_arena = engine.arena().to_vec();
        assert_eq!(
            engine.store32(0x4000, u32::MAX),
            Err(HostError::CodeInvalidated)
        );
        assert_eq!(guest_word(&engine, 0x4000), 0);
        assert_eq!(engine.arena(), stale_arena);
        assert_eq!(compile(&mut engine, 0x1000), generation + 1);
        assert_eq!(engine.store32(0x4000, 42), Ok(StoreCompletion::Complete));
        assert_eq!(guest_word(&engine, 0x4000), 42);
    }
}

#[test]
fn cross_page_fault_before_commit_keeps_covered_code_page_and_artifact_current() {
    for denied in [false, true] {
        let mut engine = with_code(0x1ffc);
        if denied {
            engine.map(0x2000, 1, 3).unwrap();
            engine.write32(0x1ffe, 0x4433_2211).unwrap();
            engine.protect(0x2000, 1, 1).unwrap();
            compile(&mut engine, 0x1ffc);
        }
        let generation = engine.generation();
        let original_word = guest_word(&engine, 0x1ffc);
        let artifact = engine.artifact_bytes().unwrap().to_vec();
        poison_helper(&mut engine);
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.store32(0x1ffe, u32::MAX),
            Ok(StoreCompletion::Complete)
        );
        assert_eq!(guest_word(&engine, 0x1ffc), original_word);
        if denied {
            assert_eq!(guest_word(&engine, 0x1ffe), 0x4433_2211);
        }
        let reason = if denied { 2 } else { 1 };
        assert_eq!(
            &engine.arena()[100..140],
            &helper_record([1, 0, reason, 0x2000, 2, 4])
        );
        assert_only_helper_changed(&engine, &before);
        assert_eq!(engine.generation(), generation);
        assert_eq!(engine.artifact_bytes().unwrap(), artifact);
        engine.guard(KEY, generation).unwrap();
    }
}

#[test]
fn final_complete_word_is_writable_but_overflow_reports_start_without_mutation() {
    let mut engine = with_code(0x1000);
    engine.map(0xffff_f000, 1, 3).unwrap();
    assert_eq!(
        engine.store32(0xffff_fffc, 0x4433_2211),
        Ok(StoreCompletion::Complete)
    );
    poison_helper(&mut engine);
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.store32(0xffff_fffd, u32::MAX),
        Ok(StoreCompletion::Complete)
    );
    assert_eq!(guest_word(&engine, 0xffff_fffc), 0x4433_2211);
    assert_eq!(
        &engine.arena()[100..140],
        &helper_record([1, 0, 3, 0xffff_fffd, 2, 4])
    );
    assert_only_helper_changed(&engine, &before);
    engine.guard(KEY, engine.generation()).unwrap();
}

#[test]
fn store_requires_write_permission_without_requiring_read_or_execute_permission() {
    let mut engine = with_code(0x1000);
    engine.map(0x4000, 1, 2).unwrap();
    assert_eq!(
        engine.store32(0x4000, 0x1122_3344),
        Ok(StoreCompletion::Complete)
    );
    assert_eq!(&engine.arena()[100..140], &helper_record([0; 6]));
    engine.protect(0x4000, 1, 1).unwrap();
    assert_eq!(guest_word(&engine, 0x4000), 0x1122_3344);
    poison_helper(&mut engine);
    assert_eq!(engine.store32(0x4000, 0), Ok(StoreCompletion::Complete));
    assert_eq!(guest_word(&engine, 0x4000), 0x1122_3344);
    assert_eq!(
        &engine.arena()[100..140],
        &helper_record([1, 0, 2, 0x4000, 2, 4])
    );
    engine.guard(KEY, engine.generation()).unwrap();
}

#[test]
fn absent_artifact_rejects_store_before_guest_access_and_helper_mutation() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(0x4000, 1, 3).unwrap();
    engine.write32(0x4000, 0x1122_3344).unwrap();
    poison_helper(&mut engine);
    let before = engine.arena().to_vec();
    for address in [0x4000, 0x8000, u32::MAX] {
        assert_eq!(
            engine.store32(address, u32::MAX),
            Err(HostError::InvalidArtifact)
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(guest_word(&engine, 0x4000), 0x1122_3344);
        assert_eq!(engine.generation(), 0);
    }
}

#[test]
fn stale_artifact_rejects_store_before_guest_access_and_helper_mutation() {
    for mutation in 0..3 {
        let mut engine = with_code(0x1000);
        engine.map(0x4000, 1, 3).unwrap();
        engine.write32(0x4000, 0x1122_3344).unwrap();
        match mutation {
            0 => engine.write32(0x1000, 0x9090_9090).unwrap(),
            1 => engine.protect(0x1000, 1, 7).unwrap(),
            2 => {
                engine.unmap(0x1000, 1).unwrap();
                engine.map(0x1000, 1, 7).unwrap();
                engine.write32(0x1000, 0x9090_9090).unwrap();
            }
            _ => unreachable!(),
        }
        poison_helper(&mut engine);
        let before = engine.arena().to_vec();
        let generation = engine.generation();
        for address in [0x4000, 0x8000, u32::MAX] {
            assert_eq!(
                engine.store32(address, u32::MAX),
                Err(HostError::CodeInvalidated)
            );
            assert_eq!(engine.arena(), before);
            assert_eq!(guest_word(&engine, 0x4000), 0x1122_3344);
            assert_eq!(engine.generation(), generation);
        }
    }
}

#[test]
fn close_gates_store_before_retained_arena_mutation() {
    let mut engine = with_code(0x1000);
    poison_helper(&mut engine);
    let before = engine.arena().to_vec();
    engine.close();
    for address in [0x1000, u32::MAX] {
        assert_eq!(engine.store32(address, u32::MAX), Err(HostError::Closed));
        assert_eq!(engine.arena(), before);
    }
}
