use ring3_engine::{
    abi::x86::{SEGMENTS_SIZE, encode_segments},
    cpu::x86::{SegmentDescriptorRecord, SegmentDescriptorTable, SegmentRegister, SegmentState},
    process::EngineInstance,
};

fn descriptor(base: u32) -> SegmentDescriptorRecord {
    SegmentDescriptorRecord::from_words(
        (base << 16) | 0xff,
        (base & 0xff00_0000) | ((base >> 16) & 0xff) | 0x0040_f200,
    )
}

#[test]
fn nonflat_data_descriptor_resolution_preserves_loaded_base_and_cached_limit() {
    for segment in [SegmentRegister::Es, SegmentRegister::Ds] {
        let mut table = SegmentDescriptorTable::default();
        let raw = descriptor(0x5000);
        table.configure_raw(0x40, raw).unwrap();
        let load = table
            .resolve_load(segment, 0x43, 3, &SegmentState::default())
            .unwrap();
        assert_eq!(load.base, 0x5000);
        assert_eq!(load.cache.limit(), 0xff);
        assert_eq!(load.selector, 0x43);
        assert_eq!(table.raw_descriptor(0x43), Some(&raw));
    }
}

#[test]
fn nonflat_data_host_configuration_keeps_other_loaded_segments_independent() {
    for segment in [SegmentRegister::Es, SegmentRegister::Ds] {
        let mut engine = EngineInstance::new(1, 0x5342).unwrap();
        let before = engine.segment_context().unwrap();
        engine.set_segment_base(segment, 0x5000).unwrap();
        let after = engine.segment_context().unwrap();
        assert_eq!(after.state.bases[segment.index()], 0x5000);
        for other in 0..6 {
            if other != segment.index() {
                assert_eq!(after.state.bases[other], before.state.bases[other]);
                assert_eq!(after.caches[other], before.caches[other]);
            }
        }
    }
}

#[test]
fn legacy_segment_codec_remains_strict_for_nonflat_data() {
    for segment in [SegmentRegister::Es, SegmentRegister::Ds] {
        let mut state = SegmentState::default();
        state.bases[segment.index()] = 0x5000;
        let mut bytes = [0xa5; SEGMENTS_SIZE];
        assert!(encode_segments(&state, &mut bytes).is_err());
        assert_eq!(bytes, [0xa5; SEGMENTS_SIZE]);
    }
}

#[test]
fn current_context_accepts_every_segment_base() {
    for segment in [
        SegmentRegister::Es,
        SegmentRegister::Cs,
        SegmentRegister::Ss,
        SegmentRegister::Ds,
        SegmentRegister::Fs,
        SegmentRegister::Gs,
    ] {
        let mut engine = EngineInstance::new(1, 0x5342).unwrap();
        engine.set_segment_base(segment, 0x5000).unwrap();
        assert_eq!(
            engine.segment_context().unwrap().state.bases[segment.index()],
            0x5000
        );
    }
}

use ring3_engine::{
    abi::{
        arena::{HELPER_OFFSET, TRANSFER_OFFSET},
        memory_helper::HELPER_SIZE,
    },
    cpu::x86::SegmentAccess,
};

const KEY: u64 = 0x5342;

fn bound_engine(resident: bool) -> (EngineInstance, Option<u64>) {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    let input = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    input[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    input[4..].copy_from_slice(&2_u32.to_le_bytes());
    let id = if resident {
        Some(engine.compile_resident(1).unwrap().get())
    } else {
        engine.compile(1).unwrap();
        None
    };
    (engine, id)
}

fn load(engine: &mut EngineInstance, id: Option<u64>, target: SegmentRegister) {
    if let Some(id) = id {
        engine
            .segment_load_resident(KEY, id, target.index() as u32, 0x43)
            .unwrap();
    } else {
        engine.segment_load(target.index() as u32, 0x43).unwrap();
    }
}

fn word(engine: &EngineInstance, offset: usize) -> u32 {
    u32::from_le_bytes(engine.arena()[offset..offset + 4].try_into().unwrap())
}

fn access_rights(engine: &mut EngineInstance, id: Option<u64>) -> u32 {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 24].fill(0);
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4]
        .copy_from_slice(&0x43_u32.to_le_bytes());
    if let Some(id) = id {
        engine.cpu_integer_resident(KEY, id, 0x30003, 0).unwrap();
    } else {
        engine.cpu_integer(0x30003, 0).unwrap();
    }
    assert_eq!(word(engine, TRANSFER_OFFSET + 4) & 0x40, 0x40);
    word(engine, TRANSFER_OFFSET)
}

#[test]
fn nonflat_bound_loads_commit_accessed_and_isolate_loaded_cache_until_reload() {
    for resident in [false, true] {
        for target in [SegmentRegister::Es, SegmentRegister::Ds] {
            let (mut engine, id) = bound_engine(resident);
            engine
                .configure_segment_descriptor_raw(0x43, descriptor(0x5000))
                .unwrap();
            assert_eq!(access_rights(&mut engine, id), 0x0040_f200);
            let before = engine.segment_context().unwrap();
            load(&mut engine, id, target);
            assert_eq!(word(&engine, HELPER_OFFSET + 16), 0);
            assert_eq!(word(&engine, HELPER_OFFSET + 20), 0x5000);
            let loaded = engine.segment_context().unwrap();
            let index = target.index();
            assert_eq!(loaded.state.selectors[index], 0x43);
            assert_eq!(loaded.state.bases[index], 0x5000);
            assert_eq!(loaded.caches[index].limit(), 0xff);
            assert!(
                loaded.caches[index]
                    .check_span(0xff, 1, SegmentAccess::Write)
                    .is_ok()
            );
            assert!(
                loaded.caches[index]
                    .check_span(0xff, 2, SegmentAccess::Read)
                    .is_err()
            );
            for other in 0..6 {
                if other != index {
                    assert_eq!(loaded.state.selectors[other], before.state.selectors[other]);
                    assert_eq!(loaded.state.bases[other], before.state.bases[other]);
                    assert_eq!(loaded.caches[other], before.caches[other]);
                }
            }
            assert_eq!(access_rights(&mut engine, id), 0x0040_f300);
            let arena = engine.arena().to_vec();
            engine
                .configure_segment_descriptor_raw(
                    0x43,
                    SegmentDescriptorRecord::from_words(0x6000_001f, 0x0040_f000),
                )
                .unwrap();
            assert_eq!(engine.arena(), arena);
            assert_eq!(engine.segment_context().unwrap(), loaded);
            assert_eq!(access_rights(&mut engine, id), 0x0040_f000);
            assert_eq!(engine.segment_context().unwrap(), loaded);
            load(&mut engine, id, target);
            let reloaded = engine.segment_context().unwrap();
            assert_eq!(reloaded.state.bases[index], 0x6000);
            assert_eq!(reloaded.caches[index].limit(), 0x1f);
            assert!(
                reloaded.caches[index]
                    .check_span(0, 1, SegmentAccess::Read)
                    .is_ok()
            );
            assert!(
                reloaded.caches[index]
                    .check_span(0, 1, SegmentAccess::Write)
                    .is_err()
            );
            assert_eq!(access_rights(&mut engine, id), 0x0040_f100);
        }
    }
}

#[test]
fn nonflat_bound_load_faults_preserve_loaded_context_and_clear_accessed_bit() {
    for resident in [false, true] {
        for target in [SegmentRegister::Es, SegmentRegister::Ds] {
            let (mut engine, id) = bound_engine(resident);
            engine
                .configure_segment_descriptor_raw(0x43, descriptor(0x5000))
                .unwrap();
            load(&mut engine, id, target);
            let loaded = engine.segment_context().unwrap();
            engine
                .configure_segment_descriptor_raw(
                    0x43,
                    SegmentDescriptorRecord::from_words(0x6000_00ff, 0x0040_7200),
                )
                .unwrap();
            assert_eq!(access_rights(&mut engine, id), 0x0040_7200);
            let mut expected = engine.arena().to_vec();
            let packet = &mut expected[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE];
            packet.fill(0);
            packet[..4].copy_from_slice(b"R3MH");
            packet[4..8].copy_from_slice(&0x0001_0007_u32.to_le_bytes());
            packet[8..12].copy_from_slice(&(HELPER_SIZE as u32).to_le_bytes());
            packet[16..20].copy_from_slice(&6_u32.to_le_bytes());
            packet[24..28].copy_from_slice(&0x40_u32.to_le_bytes());
            load(&mut engine, id, target);
            assert_eq!(engine.arena(), expected);
            assert_eq!(engine.segment_context().unwrap(), loaded);
            assert_eq!(access_rights(&mut engine, id), 0x0040_7200);
            assert_eq!(engine.segment_context().unwrap(), loaded);
        }
    }
}

#[test]
fn code_base_is_current_context_only_and_legacy_codec_refuses_it_atomically() {
    let mut engine = EngineInstance::new(1, 0x4353).unwrap();
    engine
        .set_segment_base(SegmentRegister::Cs, 0x4000)
        .unwrap();
    let context = engine.segment_context().unwrap();
    assert_eq!(context.state.bases[SegmentRegister::Cs.index()], 0x4000);
    let mut legacy = [0xa5; SEGMENTS_SIZE];
    assert!(encode_segments(&context.state, &mut legacy).is_err());
    assert_eq!(legacy, [0xa5; SEGMENTS_SIZE]);
}
