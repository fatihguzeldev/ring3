use ring3_engine::{
    abi::{
        arena::{HELPER_OFFSET, SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET, TRANSFER_OFFSET},
        cpu_helper::{CpuMemoryError, encode_cpu_result},
        x86::{
            SEGMENT_BASES_RECORD_SIZE, SEGMENTS_SIZE, decode_current_segment_context,
            encode_current_segment_context, encode_segments,
        },
    },
    cpu::x86::{
        SegmentCache, SegmentContext, SegmentDescriptorRecord, SegmentDescriptorTable,
        SegmentLoadFailure, SegmentRegister, SegmentState, StackFrameKind,
    },
    memory::{FaultReason, GuestAddress, MemoryError},
    process::{EngineInstance, StoreCompletion},
};

const KEY: u64 = 0x5353;

fn bound(resident: bool, entries: bool) -> (EngineInstance, Option<u64>) {
    let mut engine = EngineInstance::new(4, KEY).unwrap();
    for page in [0, 0x1000, 0x3000, 0x8000] {
        engine
            .map(page, 1, if page == 0x1000 { 7 } else { 3 })
            .unwrap();
    }
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    let input = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    input[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    input[4..].copy_from_slice(&2_u32.to_le_bytes());
    let id = match (resident, entries) {
        (false, false) => {
            engine.compile(1).unwrap();
            None
        }
        (false, true) => {
            engine.compile_entries(1, 0).unwrap();
            None
        }
        (true, false) => Some(engine.compile_resident(1).unwrap().get()),
        (true, true) => Some(engine.compile_resident_entries(1, 0).unwrap().get()),
    };
    (engine, id)
}

fn input(engine: &mut EngineInstance, registers: [u32; 8]) {
    for (index, value) in registers.into_iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&value.to_le_bytes());
    }
}

fn execute(
    engine: &mut EngineInstance,
    id: Option<u64>,
    kind: StackFrameKind,
    allocation: u16,
    nesting: u8,
) {
    let control = kind.control(allocation, nesting);
    let status = if let Some(id) = id {
        engine.stack_execute_resident(KEY, id, control)
    } else {
        engine.stack_execute(control)
    };
    assert_eq!(status, Ok(StoreCompletion::Complete));
}

fn packet(length: u32, result: Result<(), CpuMemoryError>) -> [u8; 40] {
    let mut bytes = [0; 40];
    encode_cpu_result(length, 0, result, &mut bytes).unwrap();
    bytes
}

fn ram(engine: &EngineInstance, address: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    bytes
}

#[test]
fn ss_only_base_uses_v3_companion_without_weakening_legacy_or_b16_validation() {
    let mut context = SegmentContext::default();
    assert!(context.state.set_base(SegmentRegister::Ss, 0x5000));
    let mut segments = [0; SEGMENTS_SIZE];
    let mut bases = [0; SEGMENT_BASES_RECORD_SIZE];
    encode_current_segment_context(&context, &mut segments, &mut bases).unwrap();
    assert_eq!(&segments[4..6], &3_u16.to_le_bytes());
    assert_eq!(&bases[24..28], &0x5000_u32.to_le_bytes());
    assert_eq!(
        decode_current_segment_context(&segments, Some(&bases)),
        Ok(context)
    );
    assert!(decode_current_segment_context(&segments, None).is_err());
    let mut legacy = [0xa5; SEGMENTS_SIZE];
    assert!(encode_segments(&context.state, &mut legacy).is_err());
    assert_eq!(legacy, [0xa5; SEGMENTS_SIZE]);
    context.caches[2] = SegmentCache::from_bits(context.caches[2].bits() & !0x0200_0000).unwrap();
    let before = (segments, bases);
    assert!(encode_current_segment_context(&context, &mut segments, &mut bases).is_err());
    assert_eq!((segments, bases), before);
}

#[test]
fn raw_nonflat_ss_resolves_without_mutating_table_and_preserves_fault_priority() {
    let mut table = SegmentDescriptorTable::empty();
    table
        .configure_raw(
            0x43,
            SegmentDescriptorRecord::from_words(0x5000_003f, 0x0040_f200),
        )
        .unwrap();
    let before = table.clone();
    let load = table
        .resolve_load(SegmentRegister::Ss, 0x43, 3, &SegmentState::default())
        .unwrap();
    assert_eq!(
        (load.base, load.cache.limit(), load.selector),
        (0x5000, 0x3f, 0x43)
    );
    assert_eq!(table, before);
    for (high, failure) in [
        (0x0040_f000, SegmentLoadFailure::Type),
        (0x0040_d200, SegmentLoadFailure::Privilege),
        (0x0040_7200, SegmentLoadFailure::NotPresent),
        (0x0000_f200, SegmentLoadFailure::UnsupportedProfile),
    ] {
        table
            .configure_raw(0x43, SegmentDescriptorRecord::from_words(0x5000_003f, high))
            .unwrap();
        assert_eq!(
            table
                .resolve_load(SegmentRegister::Ss, 0x43, 3, &SegmentState::default())
                .unwrap_err()
                .failure,
            failure
        );
    }
}

#[test]
fn nonflat_batch_push_and_enter_publish_logical_registers_and_translated_ram() {
    for resident in [false, true] {
        for entries in [false, true] {
            let (mut engine, id) = bound(resident, entries);
            engine
                .set_segment_base(SegmentRegister::Ss, 0x5000)
                .unwrap();
            let registers = [
                0x11112222, 0x33334444, 0x55556666, 0x77778888, 0x3080, 0x3080, 0x9999aaaa,
                0xbbbbcccc,
            ];
            input(&mut engine, registers);
            let before = engine.arena().to_vec();
            let decoy = ram(&engine, 0x3000, 128);
            execute(&mut engine, id, StackFrameKind::PushAll32, 0, 0);
            let mut expected = before;
            expected[HELPER_OFFSET..HELPER_OFFSET + 40].copy_from_slice(&packet(0, Ok(())));
            expected[TRANSFER_OFFSET + 16..TRANSFER_OFFSET + 20]
                .copy_from_slice(&0x3060_u32.to_le_bytes());
            assert_eq!(engine.arena(), expected);
            assert_eq!(ram(&engine, 0x3000, 128), decoy);
            let words: Vec<u8> = [
                0xbbbbcccc_u32,
                0x9999aaaa,
                0x3080,
                0x3080,
                0x77778888,
                0x55556666,
                0x33334444,
                0x11112222,
            ]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
            assert_eq!(ram(&engine, 0x8060, 32), words);
            input(&mut engine, registers);
            execute(&mut engine, id, StackFrameKind::Enter32, 16, 3);
            let result = &engine.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + 32];
            assert_eq!(&result[16..24], &[0x60, 0x30, 0, 0, 0x7c, 0x30, 0, 0]);
            assert_eq!(
                ram(&engine, 0x8070, 16),
                [0x307c_u32, 0x3080, 0x3080, 0x3080]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn translated_late_page_fault_and_logical_ss_fault_leave_all_ram_and_registers_unchanged() {
    for resident in [false, true] {
        let (mut engine, id) = bound(resident, false);
        engine
            .set_segment_base(SegmentRegister::Ss, 0x5000)
            .unwrap();
        input(&mut engine, [1, 2, 3, 4, 0x3004, 6, 7, 8]);
        let before = engine.arena().to_vec();
        let before_ram = ram(&engine, 0x8000, 128);
        execute(&mut engine, id, StackFrameKind::PushAll32, 0, 0);
        let error = MemoryError::Fault(ring3_engine::memory::MemoryFault {
            address: GuestAddress(0x7ffc),
            access: ring3_engine::memory::Access::Write,
            reason: FaultReason::Unmapped,
        });
        assert!(
            matches!(error, MemoryError::Fault(fault) if fault.address == GuestAddress(0x7ffc)
            && fault.reason == FaultReason::Unmapped)
        );
        let mut expected = before;
        expected[HELPER_OFFSET..HELPER_OFFSET + 40]
            .copy_from_slice(&packet(4, Err(CpuMemoryError::Memory(error))));
        assert_eq!(engine.arena(), expected);
        assert_eq!(ram(&engine, 0x8000, 128), before_ram);
        let mut context = engine.segment_context().unwrap();
        context.caches[2] = SegmentCache::from_bits(0xf230_003f).unwrap();
        let mut segments = [0; SEGMENTS_SIZE];
        let mut bases = [0; SEGMENT_BASES_RECORD_SIZE];
        encode_current_segment_context(&context, &mut segments, &mut bases).unwrap();
        engine.arena_mut().unwrap()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE]
            .copy_from_slice(&segments);
        engine.arena_mut().unwrap()
            [SEGMENT_BASES_RECORD_OFFSET..SEGMENT_BASES_RECORD_OFFSET + SEGMENT_BASES_RECORD_SIZE]
            .copy_from_slice(&bases);
        input(&mut engine, [1, 2, 3, 4, 0x3004, 6, 7, 8]);
        let mut expected = engine.arena().to_vec();
        expected[HELPER_OFFSET..HELPER_OFFSET + 40]
            .copy_from_slice(&packet(4, Err(CpuMemoryError::StackSegment(0))));
        execute(&mut engine, id, StackFrameKind::PushAll32, 0, 0);
        assert_eq!(engine.arena(), expected);
        assert_eq!(ram(&engine, 0x8000, 128), before_ram);
    }
}

#[test]
fn base_addition_wraps_before_a_valid_contiguous_ram_span_without_changing_logical_esp() {
    let (mut engine, id) = bound(true, true);
    engine
        .set_segment_base(SegmentRegister::Ss, 0xffff_fff0)
        .unwrap();
    input(&mut engine, [1, 2, 3, 4, 0x34, 6, 7, 8]);
    execute(&mut engine, id, StackFrameKind::PushAll32, 0, 0);
    assert_eq!(
        &engine.arena()[TRANSFER_OFFSET + 16..TRANSFER_OFFSET + 20],
        &0x14_u32.to_le_bytes()
    );
    assert_eq!(
        ram(&engine, 4, 32),
        [8_u32, 7, 6, 0x34, 4, 3, 2, 1]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>()
    );
}

#[test]
fn far_call_checks_logical_slots_then_writes_based_slots_without_publishing_cpu_registers() {
    for resident in [false, true] {
        for word in [0, 1] {
            let (mut engine, id) = bound(resident, false);
            engine
                .set_segment_base(SegmentRegister::Ss, 0x5000)
                .unwrap();
            let before = engine.arena()[..56].to_vec();
            let result = if let Some(id) = id {
                engine.far_call_resident(KEY, id, 0x1000, 0x1b, word, 0xabcd1234, 0x3080)
            } else {
                engine.far_call(0x1000, 0x1b, word, 0xabcd1234, 0x3080)
            };
            assert_eq!(result, Ok(StoreCompletion::Complete));
            assert_eq!(&engine.arena()[..56], before);
            assert_eq!(
                ram(
                    &engine,
                    if word == 1 { 0x807c } else { 0x8078 },
                    if word == 1 { 4 } else { 8 }
                ),
                if word == 1 {
                    vec![0x34, 0x12, 0x1b, 0]
                } else {
                    vec![0x34, 0x12, 0xcd, 0xab, 0x1b, 0, 0, 0]
                }
            );
            assert_eq!(ram(&engine, 0x3078, 8), vec![0; 8]);
            assert_eq!(engine.segment_context().unwrap().state.bases[2], 0x5000);
        }
    }
}

#[test]
fn host_ss_load_publishes_complete_companion_and_b16_refusal_is_arena_neutral() {
    for resident in [false, true] {
        let (mut engine, id) = bound(resident, false);
        engine
            .configure_segment_descriptor_raw(
                0x43,
                SegmentDescriptorRecord::from_words(0x5000_003f, 0x0040_f200),
            )
            .unwrap();
        let before = engine.segment_context().unwrap();
        if let Some(id) = id {
            engine.segment_load_resident(KEY, id, 2, 0x43).unwrap();
        } else {
            engine.segment_load(2, 0x43).unwrap();
        }
        let loaded = engine.segment_context().unwrap();
        assert_eq!(
            (
                loaded.state.selectors[2],
                loaded.state.bases[2],
                loaded.caches[2].bits()
            ),
            (0x43, 0x5000, 0xf230_003f)
        );
        for index in [0, 1, 3, 4, 5] {
            assert_eq!(loaded.state.bases[index], before.state.bases[index]);
            assert_eq!(loaded.caches[index], before.caches[index]);
        }
        assert_eq!(
            &engine.arena()[SEGMENTS_OFFSET + 4..SEGMENTS_OFFSET + 6],
            &3_u16.to_le_bytes()
        );
        assert_eq!(
            &engine.arena()[SEGMENT_BASES_RECORD_OFFSET + 24..SEGMENT_BASES_RECORD_OFFSET + 28],
            &0x5000_u32.to_le_bytes()
        );
        engine
            .configure_segment_descriptor_raw(
                0x43,
                SegmentDescriptorRecord::from_words(0x6000_003f, 0x0000_f200),
            )
            .unwrap();
        let before = engine.arena().to_vec();
        let result = if let Some(id) = id {
            engine.segment_load_resident(KEY, id, 2, 0x43)
        } else {
            engine.segment_load(2, 0x43)
        };
        assert_eq!(
            result,
            Err(ring3_engine::process::HostError::InvalidRequest)
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.segment_context().unwrap(), loaded);
    }
}

#[test]
fn flat_windows_call_and_callback_admission_reject_nonflat_ss_before_unmapped_stack_access() {
    use ring3_engine::{
        abi::x86::{encode_exit_v3, encode_state},
        cpu::{ExecutionExit, ExitReason, x86::State32},
        process::{CallError, HostError},
        windows::CallingConvention32,
    };
    for resident in [false, true] {
        let mut engine = EngineInstance::new(1, KEY).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
            .copy_from_slice(&[0x0f, 0x0b]);
        engine.upload(0x1000, 2).unwrap();
        for (index, value) in [0x1000_u32, 2, 0x1000, 77].into_iter().enumerate() {
            engine.arena_mut().unwrap()
                [TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
                .copy_from_slice(&value.to_le_bytes());
        }
        let id = if resident {
            Some(engine.compile_resident_with_gates(1, 1).unwrap().get())
        } else {
            engine.compile_with_gates(1, 1).unwrap();
            None
        };
        engine
            .set_segment_base(SegmentRegister::Ss, 0x5000)
            .unwrap();
        let state = State32 {
            registers: [0, 0, 0, 0, 0x24, 0, 0, 0],
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
        let capture = if let Some(id) = id {
            engine.capture_resident_call(KEY, id, CallingConvention32::Cdecl, 0)
        } else {
            engine.capture_call(KEY, engine.generation(), CallingConvention32::Cdecl, 0)
        };
        assert_eq!(capture, Err(HostError::Call(CallError::InvalidRequest)));
        assert_eq!(engine.arena(), before);
        let callback = if let Some(id) = id {
            engine
                .begin_resident_callback(KEY, id, id, 0, 0x1000, 0x1000, 78, &[])
                .map(|_| ())
        } else {
            engine
                .begin_callback(KEY, engine.generation(), 0, 0x1000, 0x1000, 78, &[])
                .map(|_| ())
        };
        assert_eq!(callback, Err(HostError::Call(CallError::InvalidRequest)));
        assert_eq!(engine.arena(), before);
    }
}
