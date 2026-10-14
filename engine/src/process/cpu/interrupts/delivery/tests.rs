use super::*;
use crate::{
    abi::{
        arena::{SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET, TRANSFER_OFFSET},
        x86::encode_current_segment_context,
    },
    cpu::x86::{
        DebugCause32, InterruptDescriptorTable32, SegmentCache, SegmentContext,
        SegmentDescriptorRecord, TrapPoint32,
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
};

const KEY: u64 = 0x45564e54;
fn gate(kind: u8, dpl: u8) -> SegmentDescriptorRecord {
    SegmentDescriptorRecord::from_words(
        0x00402345,
        (u32::from(kind) << 8) | (u32::from(dpl) << 13) | 0x8000,
    )
}
fn engine(kind: u8, dpl: u8, resident: bool) -> (EngineInstance, Option<u64>) {
    let mut engine = EngineInstance::new(8, KEY).unwrap();
    let mut records = Box::new([SegmentDescriptorRecord::from_words(0, 0); 256]);
    for vector in [1, 3, 6, 13, 14] {
        records[vector] = gate(kind, dpl);
    }
    engine
        .configure_interrupt_descriptor_table(InterruptDescriptorTable32::new(0xffff, records))
        .unwrap();
    engine
        .configure_segment_descriptor_raw(
            0x40,
            SegmentDescriptorRecord::from_words(0xffff, 0x00cffa00),
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
    origin: EventOrigin32,
    esp: u32,
) -> Result<EventDeliveryResult32, HostError> {
    let input = FrameInput32 {
        eip: 0x12345678,
        esp,
        flags: 0x254fd7,
        cs: 0x1b,
    };
    match id {
        Some(id) => engine.deliver_exception_resident(KEY, id, origin, input),
        None => engine.deliver_exception(origin, input),
    }
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
// literal protocol oracle: it does not call the production encoder or read observed packet fields.
fn packet(fields: [u32; 6]) -> [u8; HELPER_SIZE] {
    let mut bytes = [0; HELPER_SIZE];
    bytes[..16].copy_from_slice(b"R3MH\x0c\0\x01\0\x28\0\0\0\0\0\0\0");
    for (index, field) in fields.into_iter().enumerate() {
        bytes[16 + index * 4..20 + index * 4].copy_from_slice(&field.to_le_bytes());
    }
    bytes
}
fn only_packet(before: &[u8], engine: &EngineInstance, fields: [u32; 6]) {
    let mut want = before.to_vec();
    want[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE].copy_from_slice(&packet(fields));
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
fn gp() -> EventOrigin32 {
    EventOrigin32::Exception(ExceptionCause32::GeneralProtection(
        SelectorErrorCode32::descriptor(0x4b, false),
    ))
}

#[test]
fn native_attempt_commits_literal_three_and_four_slot_frames_without_live_state_or_retirement() {
    for resident in [false, true] {
        for (kind, b32, origin, frame, next_esp, live_flags, info) in [
            (
                14,
                true,
                gp(),
                vec![
                    0x48, 0, 0, 0, 0x78, 0x56, 0x34, 0x12, 0x1b, 0, 0, 0, 0xd7, 0x4f, 0x25, 0,
                ],
                0x77f1,
                0x240cd7,
                0x304,
            ),
            (
                7,
                false,
                EventOrigin32::Exception(ExceptionCause32::InvalidOpcode),
                vec![0x78, 0x56, 0x1b, 0, 0xd7, 0x4f],
                0xabcd77fb,
                0x240ed7,
                2,
            ),
        ] {
            let (mut engine, id) = engine(kind, 3, resident);
            if !b32 {
                let mut context = engine.segment_context().unwrap();
                context.caches[2] = SegmentCache::from_bits(0xf13fffff).unwrap();
                set_context(&mut engine, context);
            }
            let before = engine.arena().to_vec();
            let code = ram(&engine, 0x1000);
            let mut want_ram = ram(&engine, 0x7000);
            let offset = (next_esp & 0xffff) as usize - 0x7000;
            want_ram[offset..offset + frame.len()].copy_from_slice(&frame);
            let result = invoke(
                &mut engine,
                id,
                origin,
                if b32 { 0x7801 } else { 0xabcd7801 },
            );
            assert_eq!(
                result,
                Ok(EventDeliveryResult32::Entered(EnteredEvent32 {
                    target: 0x2345,
                    esp: next_esp,
                    flags: live_flags,
                    frame_info: info,
                    code_invalidated: false,
                }))
            );
            assert_eq!(ram(&engine, 0x7000), want_ram);
            assert_eq!(ram(&engine, 0x1000), code);
            let mut want = before;
            want[SEGMENTS_OFFSET + 18..SEGMENTS_OFFSET + 20]
                .copy_from_slice(&0x43_u16.to_le_bytes());
            want[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE]
                .copy_from_slice(&packet([0, 0x2345, 0, 0, 0, info]));
            assert_eq!(
                engine.arena(),
                want,
                "full arena: only CS and helper packet change"
            );
            assert_eq!(
                engine.segment_context().unwrap().caches[1].bits(),
                0xf3bfffff
            );
            assert_eq!(
                engine
                    .segment_descriptors
                    .raw_descriptor(0x40)
                    .unwrap()
                    .descriptor_type(),
                11
            );
        }
    }
}

#[test]
fn processor_origins_bypass_gate_dpl_while_cc_keeps_software_dpl_and_ext_zero() {
    let (mut engine, id) = engine(15, 0, false);
    let before = engine.arena().to_vec();
    let table = engine.segment_descriptors.clone();
    let code = ram(&engine, 0x1000);
    let stack = ram(&engine, 0x7000);
    assert_eq!(
        invoke(
            &mut engine,
            id,
            EventOrigin32::Exception(ExceptionCause32::BreakpointInt3),
            0x7800
        ),
        Ok(EventDeliveryResult32::ArchitecturalSecondary(
            ExceptionCause32::GeneralProtection(SelectorErrorCode32::idt(3, false))
        ))
    );
    only_packet(&before, &engine, [3, 0, 0x1a, 0, 0, 0]);
    assert_eq!(engine.segment_descriptors, table);
    assert_eq!(ram(&engine, 0x1000), code);
    assert_eq!(ram(&engine, 0x7000), stack);
    assert!(matches!(
        invoke(
            &mut engine,
            id,
            EventOrigin32::Exception(ExceptionCause32::Debug(DebugCause32::Int1)),
            0x7800
        ),
        Ok(EventDeliveryResult32::Entered(_))
    ));
    let frame = ram(&engine, 0x7000);
    assert_eq!(
        &frame[0x7f4..0x800],
        &[0x78, 0x56, 0x34, 0x12, 0x1b, 0, 0, 0, 0xd7, 0x4f, 0x24, 0]
    );
}

#[test]
fn secondary_presence_and_ss_capacity_preserve_origin_ext_and_precede_target_limit_or_pages() {
    for (high, ss, esp, status, detail, cause) in [
        (
            0x00407a00,
            0xf23077ff,
            0x7808,
            6,
            0x41,
            ExceptionCause32::SegmentNotPresent(SelectorErrorCode32::descriptor(0x40, true)),
        ),
        (
            0x0040fa00,
            0xf23077ff,
            0x7808,
            7,
            1,
            ExceptionCause32::StackSegment(SelectorErrorCode32::null(true)),
        ),
        (
            0x0040fa00,
            0xf33fffff,
            0x700c,
            3,
            1,
            ExceptionCause32::GeneralProtection(SelectorErrorCode32::null(true)),
        ),
    ] {
        let (mut engine, id) = engine(14, 3, false);
        engine
            .configure_segment_descriptor_raw(0x40, SegmentDescriptorRecord::from_words(0xff, high))
            .unwrap();
        let mut context = engine.segment_context().unwrap();
        context.caches[2] = SegmentCache::from_bits(ss).unwrap();
        set_context(&mut engine, context);
        let before = engine.arena().to_vec();
        let table = engine.segment_descriptors.clone();
        let code = ram(&engine, 0x1000);
        let stack = ram(&engine, 0x7000);
        let version = engine.memory().unwrap().code_validation_version();
        assert_eq!(
            invoke(&mut engine, id, gp(), esp),
            Ok(EventDeliveryResult32::ArchitecturalSecondary(cause))
        );
        only_packet(&before, &engine, [status, 0, detail, 0, 0, 0x304]);
        assert_eq!(engine.segment_descriptors, table);
        assert_eq!(ram(&engine, 0x1000), code);
        assert_eq!(ram(&engine, 0x7000), stack);
        assert_eq!(engine.memory().unwrap().code_validation_version(), version);
    }
}

#[test]
fn fourth_slot_page_and_version_failures_leave_every_frame_cache_and_version_unpublished() {
    for resident in [false, true] {
        for exhausted in [false, true] {
            let (mut engine, id) = engine(14, 3, resident);
            if exhausted {
                engine.memory.as_mut().unwrap().exhaust_versions_for_test();
            }
            let before = engine.arena().to_vec();
            let table = engine.segment_descriptors.clone();
            let code = ram(&engine, 0x1000);
            let stack = ram(&engine, 0x7000);
            let version = engine.memory().unwrap().code_validation_version();
            let expected = if exhausted {
                EventDeliveryResult32::Infrastructure(MemoryError::VersionExhausted)
            } else {
                EventDeliveryResult32::NeedsPagingAuthority {
                    fault: MemoryFault {
                        address: GuestAddress(0x6ffc),
                        access: Access::Write,
                        reason: FaultReason::Unmapped,
                    },
                    length: 4,
                }
            };
            assert_eq!(
                invoke(
                    &mut engine,
                    id,
                    gp(),
                    if exhausted { 0x7800 } else { 0x700c }
                ),
                Ok(expected)
            );
            only_packet(
                &before,
                &engine,
                if exhausted {
                    [2, 0, 1, 0, 0, 0x304]
                } else {
                    [1, 0, 1, 0x6ffc, 2, 0x304]
                },
            );
            assert_eq!(engine.segment_descriptors, table);
            assert_eq!(ram(&engine, 0x1000), code);
            assert_eq!(ram(&engine, 0x7000), stack);
            assert_eq!(engine.memory().unwrap().code_validation_version(), version);
        }
    }
}

#[test]
fn task_and_privilege_authority_are_typed_and_guard_failures_are_all_state_neutral() {
    for (kind, high, authority, info) in [
        (5, 0x00cffa00, EventAuthorityKind32::Task, 0),
        (14, 0x00cf9a00, EventAuthorityKind32::Privilege, 0x304),
    ] {
        let (mut engine, id) = engine(kind, 3, true);
        engine
            .configure_segment_descriptor_raw(
                0x40,
                SegmentDescriptorRecord::from_words(0xffff, high),
            )
            .unwrap();
        let before = engine.arena().to_vec();
        let table = engine.segment_descriptors.clone();
        let code = ram(&engine, 0x1000);
        let stack = ram(&engine, 0x7000);
        let version = engine.memory().unwrap().code_validation_version();
        let input = FrameInput32 {
            eip: 0x1200,
            esp: 0x7800,
            flags: 2,
            cs: 0x1b,
        };
        assert!(
            engine
                .deliver_exception_resident(KEY ^ 1, id.unwrap(), gp(), input)
                .is_err()
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(
            invoke(&mut engine, id, gp(), 0x7800),
            Ok(EventDeliveryResult32::NeedsTaskOrPrivilegeAuthority {
                kind: authority,
                selector: 0x40,
            })
        );
        only_packet(
            &before,
            &engine,
            [
                8,
                0,
                if authority == EventAuthorityKind32::Task {
                    1
                } else {
                    2
                },
                0x40,
                0,
                info,
            ],
        );
        assert_eq!(engine.segment_descriptors, table);
        assert_eq!(ram(&engine, 0x1000), code);
        assert_eq!(ram(&engine, 0x7000), stack);
        assert_eq!(engine.memory().unwrap().code_validation_version(), version);
    }
}

#[test]
fn complete_frame_smc_uses_physical_material_currentness_and_never_a_new_cs_key() {
    for resident in [false, true] {
        for (page, invalidated) in [(0x1000, true), (0x3000, false)] {
            let (mut engine, id) = engine(15, 3, resident);
            if page == 0x3000 {
                engine.map(page, 1, 7).unwrap();
            }
            let result = invoke(&mut engine, id, gp(), page + 16).unwrap();
            assert_eq!(
                result,
                EventDeliveryResult32::Entered(EnteredEvent32 {
                    target: 0x2345,
                    esp: page,
                    flags: 0x240ed7,
                    frame_info: 0x204,
                    code_invalidated: invalidated,
                })
            );
            assert_eq!(
                &ram(&engine, page)[..16],
                &[
                    0x48, 0, 0, 0, 0x78, 0x56, 0x34, 0x12, 0x1b, 0, 0, 0, 0xd7, 0x4f, 0x25, 0
                ]
            );
            assert_eq!(
                &engine.arena()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE],
                &packet([0, 0x2345, 0, 0, 0, 0x204])
            );
            assert_eq!(engine.segment_context().unwrap().state.selectors[1], 0x43);
        }
    }
}

#[test]
fn precise_trap_point_is_supplied_without_recomputing_a_syntactic_next_pc() {
    let (mut engine, id) = engine(15, 3, false);
    let input = FrameInput32 {
        eip: 0x1200,
        esp: 0x7800,
        flags: 2,
        cs: 0x1b,
    };
    let origin = EventOrigin32::Exception(ExceptionCause32::Debug(DebugCause32::SingleStep(
        TrapPoint32::CompletedInstruction,
    )));
    assert!(matches!(
        engine.deliver_exception(origin, input),
        Ok(EventDeliveryResult32::Entered(_))
    ));
    let frame = ram(&engine, 0x7000);
    assert_eq!(
        &frame[0x7f4..0x800],
        &[0, 0x12, 0, 0, 0x1b, 0, 0, 0, 2, 0, 0, 0]
    );
    let _ = id;
}
