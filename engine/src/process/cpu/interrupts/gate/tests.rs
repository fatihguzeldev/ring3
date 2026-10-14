use super::*;
use crate::{
    abi::{
        arena::{SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET, TRANSFER_OFFSET},
        x86::encode_current_segment_context,
    },
    cpu::x86::{InterruptDescriptorTable32, SegmentCache, SegmentDescriptorRecord},
    memory::GuestAddress,
};

const VECTOR: u8 = 0x80;
const KEY: u64 = 0x494e5447;
fn gate(kind: u8, selector: u16, offset: u32, present: bool) -> SegmentDescriptorRecord {
    SegmentDescriptorRecord::from_words(
        (u32::from(selector) << 16) | (offset & 0xffff),
        (if matches!(kind, 14 | 15) {
            offset & 0xffff0000
        } else {
            0
        }) | (u32::from(kind) << 8)
            | 0x6000
            | (u32::from(present) << 15),
    )
}
fn engine(
    kind: u8,
    target: u32,
    target_high: u32,
    resident: bool,
) -> (EngineInstance, Option<u64>) {
    let mut engine = EngineInstance::new(8, KEY).unwrap();
    let mut records = Box::new([SegmentDescriptorRecord::from_words(0, 0); 256]);
    records[usize::from(VECTOR)] = gate(kind, 0x40, target, true);
    engine
        .configure_interrupt_descriptor_table(InterruptDescriptorTable32::new(0x407, records))
        .unwrap();
    engine
        .configure_segment_descriptor_raw(
            0x40,
            SegmentDescriptorRecord::from_words(0x5000ffff, target_high),
        )
        .unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x7000, 1, 3).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    let id = if resident {
        Some(engine.compile_resident(1).unwrap().get())
    } else {
        engine.compile(1).unwrap();
        None
    };
    (engine, id)
}
fn invoke(
    engine: &mut EngineInstance,
    id: Option<u64>,
    esp: u32,
    flags: u32,
) -> Result<StoreCompletion, HostError> {
    match id {
        Some(id) => {
            engine.interrupt_gate_resident(KEY, id, u32::from(VECTOR), 0xabcd1234, esp, flags)
        }
        None => engine.interrupt_gate(u32::from(VECTOR), 0xabcd1234, esp, flags),
    }
}
fn field(engine: &EngineInstance, offset: usize) -> u32 {
    u32::from_le_bytes(
        engine.arena()[HELPER_OFFSET + offset..HELPER_OFFSET + offset + 4]
            .try_into()
            .unwrap(),
    )
}
fn ram(engine: &EngineInstance, page: u32) -> Vec<u8> {
    let mut bytes = vec![0; 4096];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(page), &mut bytes)
        .unwrap();
    bytes
}
fn only_packet(before: &[u8], engine: &EngineInstance) {
    let mut want = before.to_vec();
    want[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE]
        .copy_from_slice(&engine.arena()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE]);
    assert_eq!(engine.arena(), want);
}
fn set_context(engine: &mut EngineInstance, context: SegmentContext) {
    let mut segments = [0; SEGMENTS_SIZE];
    let mut bases = [0; SEGMENT_BASES_RECORD_SIZE];
    encode_current_segment_context(&context, &mut segments, &mut bases).unwrap();
    engine.arena_mut().unwrap()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE]
        .copy_from_slice(&segments);
    engine.arena_mut().unwrap()
        [SEGMENT_BASES_RECORD_OFFSET..SEGMENT_BASES_RECORD_OFFSET + SEGMENT_BASES_RECORD_SIZE]
        .copy_from_slice(&bases);
}

#[test]
fn exact_gate_selected_frames_save_rf_zero_and_keep_stack_address_size_independent() {
    for resident in [false, true] {
        for (kind, b32, slot, info) in [
            (7, false, 2, 2),
            (14, true, 4, 0x104),
            (15, false, 4, 4),
            (6, true, 2, 0x102),
        ] {
            let (mut engine, id) = engine(kind, 0x2345, 0x00cffa00, resident);
            if !b32 {
                let mut context = engine.segment_context().unwrap();
                context.caches[2] =
                    SegmentCache::from_bits(context.caches[2].bits() & !(1 << 25)).unwrap();
                set_context(&mut engine, context);
            }
            let esp = if b32 { 0x7801 } else { 0xabcd7801 };
            let before = engine.arena().to_vec();
            assert_eq!(
                invoke(&mut engine, id, esp, 0x254fd7),
                Ok(StoreCompletion::Complete)
            );
            let mut frame = vec![0; (3 * slot) as usize];
            engine
                .memory()
                .unwrap()
                .read(GuestAddress(0x7801 - 3 * slot), &mut frame)
                .unwrap();
            let expected = [0xabcd1234_u32, 0x1b, 0x244fd7]
                .into_iter()
                .flat_map(|value| value.to_le_bytes()[..slot as usize].to_vec())
                .collect::<Vec<_>>();
            assert_eq!(
                frame, expected,
                "return IP, CS, FLAGS in ascending memory order"
            );
            assert_eq!(
                &engine.arena()[..56],
                &before[..56],
                "helper leaves generated live-state commit to its caller"
            );
            assert_eq!(
                (field(&engine, 16), field(&engine, 20), field(&engine, 36)),
                (0, 0x2345, info)
            );
            assert_eq!(
                &engine.arena()[HELPER_OFFSET..HELPER_OFFSET + 16],
                b"R3MH\x0b\0\x01\0\x28\0\0\0\0\0\0\0"
            );
            let context = engine.segment_context().unwrap();
            assert_eq!(
                (
                    context.state.selectors[1],
                    context.state.bases[1],
                    context.caches[1].bits()
                ),
                (0x43, 0x5000, 0xf3bfffff)
            );
            assert_eq!(
                engine
                    .segment_descriptors
                    .raw_descriptor(0x40)
                    .unwrap()
                    .descriptor_type()
                    & 1,
                1
            );
        }
    }
}

#[test]
fn target_presence_precedes_ss_capacity_and_ss_capacity_precedes_target_limit() {
    let (mut engine, id) = engine(14, 0x100, 0x0040fa00, false);
    engine
        .configure_segment_descriptor_raw(
            0x40,
            SegmentDescriptorRecord::from_words(0xff, 0x0040fa00),
        )
        .unwrap();
    let mut context = engine.segment_context().unwrap();
    context.caches[2] = SegmentCache::from_bits(0xf23077ff).unwrap();
    set_context(&mut engine, context);
    for (high, expected) in [(0x0040fa00, (7, 0)), (0x00407a00, (6, 0x40))] {
        engine
            .configure_segment_descriptor_raw(0x40, SegmentDescriptorRecord::from_words(0xff, high))
            .unwrap();
        let before = engine.arena().to_vec();
        let table = engine.segment_descriptors.clone();
        let memory = ram(&engine, 0x7000);
        invoke(&mut engine, id, 0x7808, 2).unwrap();
        assert_eq!((field(&engine, 16), field(&engine, 24)), expected);
        assert_eq!(field(&engine, 36), 0x104);
        only_packet(&before, &engine);
        assert_eq!(engine.segment_descriptors, table);
        assert_eq!(ram(&engine, 0x7000), memory);
    }
    // full cached stack spans are valid, but target GP0 still precedes an unmapped third page slot.
    set_context(&mut engine, SegmentContext::default());
    engine
        .configure_segment_descriptor_raw(
            0x40,
            SegmentDescriptorRecord::from_words(0xff, 0x0040fa00),
        )
        .unwrap();
    let before = engine.arena().to_vec();
    invoke(&mut engine, id, 0x7008, 2).unwrap();
    assert_eq!((field(&engine, 16), field(&engine, 24)), (3, 0));
    only_packet(&before, &engine);
}

#[test]
fn late_third_slot_and_version_failure_publish_no_frame_cache_or_accessed_bit() {
    for resident in [false, true] {
        for exhausted in [false, true] {
            let (mut engine, id) = engine(15, 0x2345, 0x00cffa00, resident);
            if exhausted {
                engine.memory.as_mut().unwrap().exhaust_versions_for_test();
            }
            let before = engine.arena().to_vec();
            let table = engine.segment_descriptors.clone();
            let memory = ram(&engine, 0x7000);
            let version = engine.memory().unwrap().code_validation_version();
            invoke(&mut engine, id, if exhausted { 0x7800 } else { 0x7008 }, 2).unwrap();
            assert_eq!(
                (field(&engine, 16), field(&engine, 24)),
                if exhausted { (2, 1) } else { (1, 1) }
            );
            assert_eq!(field(&engine, 36), 4);
            if !exhausted {
                assert_eq!((field(&engine, 28), field(&engine, 32)), (0x6ffc, 2));
            }
            only_packet(&before, &engine);
            assert_eq!(ram(&engine, 0x7000), memory);
            assert_eq!(engine.segment_descriptors, table);
            assert_eq!(engine.memory().unwrap().code_validation_version(), version);
        }
    }
}

#[test]
fn task_and_interprivilege_returns_use_typed_neutral_unsupported() {
    for (kind, high) in [(5, 0x00cffa00), (14, 0x00cf9a00)] {
        let (mut engine, id) = engine(kind, 0x2345, high, false);
        let before = engine.arena().to_vec();
        let table = engine.segment_descriptors.clone();
        let memory = ram(&engine, 0x7000);
        invoke(&mut engine, id, 0x7800, 0x254fd7).unwrap();
        assert_eq!(
            (
                field(&engine, 16),
                field(&engine, 20),
                field(&engine, 24),
                field(&engine, 36)
            ),
            (8, 0, 4, 0)
        );
        only_packet(&before, &engine);
        assert_eq!(engine.segment_descriptors, table);
        assert_eq!(ram(&engine, 0x7000), memory);
    }
}

#[test]
fn frame_self_modification_commits_before_existing_stale_material_completion() {
    for resident in [false, true] {
        let (mut engine, id) = engine(14, 0x2345, 0x00cffa00, resident);
        assert_eq!(
            invoke(&mut engine, id, 0x100c, 0x254fd7),
            Ok(StoreCompletion::CodeInvalidated)
        );
        let mut frame = [0; 12];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x1000), &mut frame)
            .unwrap();
        assert_eq!(
            frame,
            [0x34, 0x12, 0xcd, 0xab, 0x1b, 0, 0, 0, 0xd7, 0x4f, 0x24, 0]
        );
        assert_eq!(engine.segment_context().unwrap().state.selectors[1], 0x43);
        assert_eq!((field(&engine, 16), field(&engine, 36)), (0, 0x104));
        assert_eq!(
            engine
                .segment_descriptors
                .raw_descriptor(0x40)
                .unwrap()
                .descriptor_type()
                & 1,
            1
        );
    }
}

#[test]
fn guards_and_malformed_host_arguments_precede_packet_or_memory_publication() {
    let (mut engine, id) = engine(14, 0x2345, 0x00cffa00, true);
    let id = id.unwrap();
    let before = engine.arena().to_vec();
    let memory = ram(&engine, 0x7000);
    let table = engine.segment_descriptors.clone();
    assert!(
        engine
            .interrupt_gate_resident(KEY ^ 1, id, 256, 0, 0, 0)
            .is_err()
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(ram(&engine, 0x7000), memory);
    assert_eq!(engine.segment_descriptors, table);
    for (vector, flags) in [(256, 2), (0x80, 0), (0x80, 0x2002)] {
        assert_eq!(
            engine.interrupt_gate_resident(KEY, id, vector, 0, 0, flags),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(ram(&engine, 0x7000), memory);
    }
}
