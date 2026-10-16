use ring3_engine::{
    abi::{
        arena::{SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET, TRANSFER_OFFSET},
        x86::{
            SEGMENT_BASES_RECORD_SIZE, SEGMENTS_SIZE, decode_current_segment_context,
            decode_segment_context, encode_current_segment_context, encode_segment_context,
        },
    },
    cpu::x86::{
        SegmentAccess, SegmentCache, SegmentDescriptorRecord, SegmentRegister, StackFrameKind,
    },
    memory::GuestAddress,
    process::{EngineInstance, HostError, StoreCompletion},
};
const KEY: u64 = 0x423136;
fn bound(resident: bool) -> (EngineInstance, Option<u64>) {
    let mut engine = EngineInstance::new(4, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x8000, 1, 3).unwrap();
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
fn b16(engine: &mut EngineInstance, base: u32) {
    let mut context = engine.segment_context().unwrap();
    context.state.bases[2] = base;
    context.caches[2] = SegmentCache::from_bits(context.caches[2].bits() & !0x0200_0000).unwrap();
    let mut segment = [0; SEGMENTS_SIZE];
    let mut bases = [0; SEGMENT_BASES_RECORD_SIZE];
    encode_current_segment_context(&context, &mut segment, &mut bases).unwrap();
    engine.arena_mut().unwrap()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE]
        .copy_from_slice(&segment);
    engine.arena_mut().unwrap()
        [SEGMENT_BASES_RECORD_OFFSET..SEGMENT_BASES_RECORD_OFFSET + SEGMENT_BASES_RECORD_SIZE]
        .copy_from_slice(&bases);
}
fn input(engine: &mut EngineInstance, regs: [u32; 8]) {
    for (i, v) in regs.into_iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + i * 4..TRANSFER_OFFSET + i * 4 + 4]
            .copy_from_slice(&v.to_le_bytes());
    }
}
fn output(engine: &EngineInstance) -> [u32; 8] {
    std::array::from_fn(|i| {
        u32::from_le_bytes(
            engine.arena()[TRANSFER_OFFSET + i * 4..TRANSFER_OFFSET + i * 4 + 4]
                .try_into()
                .unwrap(),
        )
    })
}
fn execute(
    engine: &mut EngineInstance,
    id: Option<u64>,
    kind: StackFrameKind,
    a: u16,
    n: u8,
) -> Result<StoreCompletion, HostError> {
    if let Some(id) = id {
        engine.stack_execute_resident(KEY, id, kind.control(a, n))
    } else {
        engine.stack_execute(kind.control(a, n))
    }
}
fn ram(engine: &EngineInstance, address: u32, len: usize) -> Vec<u8> {
    let mut data = vec![0; len];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut data)
        .unwrap();
    data
}
#[test]
fn b16_with_zero_bases_requires_strict_v3_and_legacy_v2_stays_b32() {
    let (mut engine, _) = bound(false);
    b16(&mut engine, 0);
    let context = engine.segment_context().unwrap();
    assert!(!context.caches[2].default32());
    let segment = &engine.arena()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + 64];
    let bases = &engine.arena()[SEGMENT_BASES_RECORD_OFFSET..SEGMENT_BASES_RECORD_OFFSET + 40];
    assert_eq!(&segment[4..6], &3_u16.to_le_bytes());
    assert!(decode_current_segment_context(segment, None).is_err());
    assert_eq!(
        decode_current_segment_context(segment, Some(bases)),
        Ok(context)
    );
    assert!(encode_segment_context(&context, &mut [0; 64]).is_err());
    let mut old = segment.to_vec();
    old[4..6].copy_from_slice(&2_u16.to_le_bytes());
    assert!(decode_segment_context(&old).is_err());
    assert!(decode_current_segment_context(&old, Some(&[0xa5; 40])).is_err());
}
#[test]
fn normal_b16_spans_follow_loaded_limit_while_expand_down_keeps_ffff_upper_bound() {
    let normal = SegmentCache::from_bits(0xf13f_ffff).unwrap();
    assert!(normal.check_span(0xffff, 4, SegmentAccess::Read).is_ok());
    let down = SegmentCache::from_bits(0xf070_0fff).unwrap();
    assert!(down.check_span(0xfffe, 2, SegmentAccess::Read).is_ok());
    assert!(down.check_span(0xfffe, 4, SegmentAccess::Read).is_err());
    assert!(down.check_span(0x10000, 1, SegmentAccess::Read).is_err());
}
#[test]
fn b16_bulk_operand_width_controls_slots_but_high_esp_survives_push_and_pop() {
    for resident in [false, true] {
        for word in [false, true] {
            let (mut engine, id) = bound(resident);
            b16(&mut engine, 0x5000);
            let original = [
                0x11112222, 0x33334444, 0x55556666, 0x77778888, 0xabcd3080, 0x76543080, 0x9999aaaa,
                0xbbbbcccc,
            ];
            input(&mut engine, original);
            assert_eq!(
                execute(
                    &mut engine,
                    id,
                    if word {
                        StackFrameKind::PushAll16
                    } else {
                        StackFrameKind::PushAll32
                    },
                    0,
                    0
                ),
                Ok(StoreCompletion::Complete)
            );
            let mut expected = original;
            expected[4] = if word { 0xabcd3070 } else { 0xabcd3060 };
            assert_eq!(output(&engine), expected);
            let bytes = ram(
                &engine,
                if word { 0x8070 } else { 0x8060 },
                if word { 16 } else { 32 },
            );
            assert_eq!(
                &bytes[if word { 6 } else { 12 }..if word { 8 } else { 16 }],
                &original[4].to_le_bytes()[..if word { 2 } else { 4 }]
            );
            assert_eq!(
                execute(
                    &mut engine,
                    id,
                    if word {
                        StackFrameKind::PopAll16
                    } else {
                        StackFrameKind::PopAll32
                    },
                    0,
                    0
                ),
                Ok(StoreCompletion::Complete)
            );
            assert_eq!(output(&engine), original);
        }
    }
}
#[test]
fn same_size_enter16_uses_low_bp_parent_chain_preserves_high_register_halves_and_overlay() {
    for resident in [false, true] {
        let (mut engine, id) = bound(resident);
        b16(&mut engine, 0x5000);
        let original = [1, 2, 3, 4, 0x12343080, 0xabcd3080, 7, 8];
        input(&mut engine, original);
        assert_eq!(
            execute(&mut engine, id, StackFrameKind::Enter16, 16, 3),
            Ok(StoreCompletion::Complete)
        );
        let mut expected = original;
        expected[4] = 0x12343068;
        expected[5] = 0xabcd307e;
        assert_eq!(output(&engine), expected);
        assert_eq!(
            ram(&engine, 0x8078, 8),
            vec![0x7e, 0x30, 0x80, 0x30, 0x80, 0x30, 0x80, 0x30]
        );
    }
}
#[test]
fn mixed_enter32_b16_uses_full_frame_value_active_parent_offsets_and_wrapped_allocation() {
    for resident in [false, true] {
        for (esp, ebp, allocation, nesting, expected_esp, expected_ebp, address, expected_words) in [
            (
                0xabcd0010,
                0x76540044,
                0x20,
                0,
                0xabcdffec,
                0xabcd000c,
                0x500c,
                vec![0x76540044_u32],
            ),
            (
                0xabcd0020,
                0x76540010,
                0,
                2,
                0xabcd0014,
                0xabcd001c,
                0x5014,
                vec![0xabcd001c, 0x11112222, 0x76540010],
            ),
            (
                0xabcd0020,
                0x76540020,
                0x10,
                3,
                0xabcd0000,
                0xabcd001c,
                0x5010,
                vec![0xabcd001c, 0x76540020, 0x76540020, 0x76540020],
            ),
            (
                0xabcd0004,
                0x76540004,
                0,
                2,
                0xabcdfff8,
                0xabcd0000,
                0x14ff8,
                vec![0xabcd0000, 0x76540004],
            ),
        ] {
            let (mut engine, id) = bound(resident);
            engine.map(0x5000, 1, 3).unwrap();
            engine.map(0x14000, 1, 3).unwrap();
            b16(&mut engine, 0x5000);
            engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4]
                .copy_from_slice(&0x11112222_u32.to_le_bytes());
            engine.upload(0x500c, 4).unwrap();
            let original = [1, 2, 3, 4, esp, ebp, 7, 8];
            input(&mut engine, original);
            assert_eq!(
                execute(
                    &mut engine,
                    id,
                    StackFrameKind::Enter32,
                    allocation,
                    nesting
                ),
                Ok(StoreCompletion::Complete)
            );
            let mut expected = original;
            expected[4] = expected_esp;
            expected[5] = expected_ebp;
            assert_eq!(output(&engine), expected);
            let expected_bytes: Vec<_> = expected_words
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect();
            assert_eq!(ram(&engine, address, expected_bytes.len()), expected_bytes);
            if esp == 0xabcd0004 {
                assert_eq!(ram(&engine, 0x5000, 4), 0x76540004_u32.to_le_bytes());
            }
        }
    }
}
#[test]
fn mixed_enter_final_probe_fault_keeps_all_arena_and_ram_owners_atomic() {
    use ring3_engine::abi::arena::HELPER_OFFSET;
    for resident in [false, true] {
        let (mut engine, id) = bound(resident);
        engine.map(0x6000, 1, 3).unwrap();
        b16(&mut engine, 0x5000);
        input(&mut engine, [1, 2, 3, 4, 0xabcd1004, 0x76540024, 7, 8]);
        let before = engine.arena().to_vec();
        let before_ram = ram(&engine, 0x6000, 16);
        assert_eq!(
            execute(&mut engine, id, StackFrameKind::Enter32, 8, 0),
            Ok(StoreCompletion::Complete)
        );
        let mut packet = [0_u8; 40];
        packet[..16].copy_from_slice(b"R3MH\x0f\0\x01\0\x28\0\0\0\0\0\0\0");
        for (index, value) in [9_u32, 0, 6, 0x5ff8, 2, 4].into_iter().enumerate() {
            packet[16 + index * 4..20 + index * 4].copy_from_slice(&value.to_le_bytes());
        }
        let mut expected = before;
        expected[HELPER_OFFSET..HELPER_OFFSET + 40].copy_from_slice(&packet);
        assert_eq!(engine.arena(), expected);
        assert_eq!(ram(&engine, 0x6000, 16), before_ram);
    }
}
#[test]
fn raw_b16_stack_load_promotes_zero_base_to_current_v3() {
    let (mut engine, id) = bound(true);
    engine
        .configure_segment_descriptor_raw(
            0x43,
            SegmentDescriptorRecord::from_words(0xffff, 0x0000f200),
        )
        .unwrap();
    engine
        .segment_load_resident(KEY, id.unwrap(), SegmentRegister::Ss.index() as u32, 0x43)
        .unwrap();
    let context = engine.segment_context().unwrap();
    assert_eq!(
        (
            context.state.selectors[2],
            context.state.bases[2],
            context.caches[2].bits()
        ),
        (0x43, 0, 0xf030ffff)
    );
    assert_eq!(
        &engine.arena()[SEGMENTS_OFFSET + 4..SEGMENTS_OFFSET + 6],
        &3_u16.to_le_bytes()
    );
}

#[test]
fn flat_windows_frame_boundary_rejects_b16_even_with_all_bases_zero() {
    use ring3_engine::{
        abi::x86::{encode_exit_v3, encode_state},
        cpu::{ExecutionExit, ExitReason, x86::State32},
        process::CallError,
        windows::CallingConvention32,
    };
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0x0f, 0x0b]);
    engine.upload(0x1000, 2).unwrap();
    for (i, v) in [0x1000_u32, 2, 0x1000, 77].into_iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + i * 4..TRANSFER_OFFSET + i * 4 + 4]
            .copy_from_slice(&v.to_le_bytes());
    }
    let generation = engine.compile_with_gates(1, 1).unwrap();
    b16(&mut engine, 0);
    let state = State32 {
        registers: [0, 0, 0, 0, 0xabcd0024, 0, 0, 0],
        eip: 0x1000,
        eflags: 2,
    };
    encode_state(&state, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 1,
            reason: ExitReason::Gate { id: 77 },
        },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.capture_call(KEY, generation, CallingConvention32::Cdecl, 0),
        Err(HostError::Call(CallError::InvalidRequest))
    );
    assert_eq!(engine.arena(), before);
}
