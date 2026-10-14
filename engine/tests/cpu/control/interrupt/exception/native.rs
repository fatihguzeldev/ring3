use ring3_engine::{
    abi::{
        arena::{HELPER_OFFSET, SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET, TRANSFER_OFFSET},
        x86::encode_state_v3,
    },
    cpu::x86::{
        DebugCause32, EnteredEvent32, EventDeliveryResult32, EventOrigin32, ExceptionCause32,
        FrameInput32, InterruptDescriptorTable32, LegacyPageViolation32, PageFaultProfile32,
        PagingAccess32, PagingDomain32, SegmentDescriptorRecord, SelectorErrorCode32, State32,
        TrapPoint32, VerifiedPageFault32,
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{EngineInstance, HostError},
};

const KEY: u64 = 0x4556_454e_5453;
const FLAGS: u32 = 0x0024_0ed7;

fn record(magic: &[u8; 4], size: usize, version: u16) -> Vec<u8> {
    let mut bytes = vec![0; size];
    bytes[..4].copy_from_slice(magic);
    bytes[4..6].copy_from_slice(&version.to_le_bytes());
    bytes[6..8].copy_from_slice(&1_u16.to_le_bytes());
    bytes[8..12].copy_from_slice(&(size as u32).to_le_bytes());
    bytes
}

fn packet(fields: [u32; 6]) -> Vec<u8> {
    let mut bytes = record(b"R3MH", 40, 12);
    for (index, value) in fields.into_iter().enumerate() {
        bytes[16 + index * 4..20 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn context(stack: u32, selector: u16, based: bool) -> (Vec<u8>, Vec<u8>) {
    let mut sg = record(b"R3SG", 64, 3);
    for (index, value) in [0x23_u16, selector, 0x23, 0x23, 0x3b, 0]
        .into_iter()
        .enumerate()
    {
        sg[16 + index * 2..18 + index * 2].copy_from_slice(&value.to_le_bytes());
    }
    for (offset, value) in [28, 32, 36, 40, 44, 56].into_iter().zip([
        0xf33f_ffff_u32,
        0xf3bf_ffff,
        stack,
        0xf33f_ffff,
        0xf33f_ffff,
        0,
    ]) {
        sg[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    let mut sb = record(b"R3SB", 40, 1);
    if based {
        sb[20..24].copy_from_slice(&0x5000_u32.to_le_bytes());
    }
    (sg, sb)
}

fn engine(
    resident: bool,
    entries: bool,
    vector: u8,
    kind: u32,
    dpl: u32,
    present: bool,
    target_selector: u16,
    stack: u32,
) -> (EngineInstance, Option<u64>) {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    let mut gates = Box::new([SegmentDescriptorRecord::from_words(0, 0); 256]);
    gates[usize::from(vector)] = SegmentDescriptorRecord::from_words(
        u32::from(target_selector) << 16 | 0x2345,
        u32::from(present) << 15 | dpl << 13 | kind << 8,
    );
    engine
        .configure_interrupt_descriptor_table(InterruptDescriptorTable32::new(0x7ff, gates))
        .unwrap();
    engine
        .configure_segment_descriptor_raw(
            0x40,
            SegmentDescriptorRecord::from_words(0x5000_ffff, 0x00cf_fa00),
        )
        .unwrap();
    for page in [0x1000, 0x7000] {
        engine.map(page, 1, 7).unwrap();
    }
    let (sg, sb) = context(stack, 0x1b, false);
    engine.arena_mut().unwrap()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + 64].copy_from_slice(&sg);
    engine.arena_mut().unwrap()[SEGMENT_BASES_RECORD_OFFSET..SEGMENT_BASES_RECORD_OFFSET + 40]
        .copy_from_slice(&sb);
    let mut regs = [
        0x1234_5678,
        0x2345_6789,
        0x3456_789a,
        0x4567_89ab,
        0x8000,
        0x5678_9abc,
        0x6789_abcd,
        0x789a_bcde,
    ];
    regs[4] = 0x8000;
    encode_state_v3(
        &State32 {
            registers: regs,
            eip: 0x1000,
            eflags: FLAGS,
        },
        &mut engine.arena_mut().unwrap()[..56],
    )
    .unwrap();
    engine.arena_mut().unwrap()[12..16].copy_from_slice(&1_u32.to_le_bytes());
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8].copy_from_slice(&[
        0,
        0x10,
        0,
        0,
        if entries { 0 } else { 2 },
        0,
        0,
        0,
    ]);
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

fn invoke(
    engine: &mut EngineInstance,
    id: Option<u64>,
    cause: ExceptionCause32,
    input: FrameInput32,
) -> Result<EventDeliveryResult32, HostError> {
    let origin = EventOrigin32::Exception(cause);
    match id {
        Some(id) => engine.deliver_exception_resident(KEY, id, origin, input),
        None => engine.deliver_exception(origin, input),
    }
}

fn ram(engine: &EngineInstance) -> [Vec<u8>; 2] {
    [0x1000, 0x7000].map(|page| {
        let mut bytes = vec![0; 4096];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(page), &mut bytes)
            .unwrap();
        bytes
    })
}

#[test]
fn supplied_fault_frames_distinguish_no_error_from_zero_error_and_gate_width_from_stack_b() {
    for resident in [false, true] {
        for entries in [false, true] {
            for (cause, kind, stack, esp, next_esp, info, literal) in [
                (
                    ExceptionCause32::InvalidOpcode,
                    15,
                    0xf33f_ffff,
                    0x8000,
                    0x7ff4,
                    4,
                    &[0, 0x10, 0, 0, 0x1b, 0, 0, 0, 0xd7, 0x0e, 0x25, 0][..],
                ),
                (
                    ExceptionCause32::GeneralProtection(SelectorErrorCode32::null(false)),
                    7,
                    0xf13f_ffff,
                    0xabcd_8000,
                    0xabcd_7ff8,
                    0x202,
                    &[0, 0, 0, 0x10, 0x1b, 0, 0xd7, 0x0e][..],
                ),
            ] {
                let (mut engine, id) = engine(
                    resident,
                    entries,
                    cause.vector(),
                    kind,
                    0,
                    true,
                    0x40,
                    stack,
                );
                let before = engine.arena().to_vec();
                let mut expected_ram = ram(&engine);
                let start = (next_esp & 0xffff) as usize - 0x7000;
                expected_ram[1][start..start + literal.len()].copy_from_slice(literal);
                assert_eq!(
                    invoke(
                        &mut engine,
                        id,
                        cause,
                        FrameInput32 {
                            eip: 0x1000,
                            esp,
                            flags: FLAGS,
                            cs: 0x1b
                        }
                    ),
                    Ok(EventDeliveryResult32::Entered(EnteredEvent32 {
                        target: 0x2345,
                        esp: next_esp,
                        flags: FLAGS,
                        frame_info: info,
                        code_invalidated: false,
                    }))
                );
                let mut want = before;
                let (sg, sb) = context(stack, 0x43, true);
                want[SEGMENTS_OFFSET..SEGMENTS_OFFSET + 64].copy_from_slice(&sg);
                want[SEGMENT_BASES_RECORD_OFFSET..SEGMENT_BASES_RECORD_OFFSET + 40]
                    .copy_from_slice(&sb);
                want[HELPER_OFFSET..HELPER_OFFSET + 40]
                    .copy_from_slice(&packet([0, 0x2345, 0, 0, 0, info]));
                assert_eq!(
                    engine.arena(),
                    want,
                    "native core publishes no GPR/EIP/ESP/FLAGS/shadow"
                );
                assert_eq!(ram(&engine), expected_ram);
            }
        }
    }
}

#[test]
fn software_breakpoint_dpl_and_processor_secondary_ext_are_distinct() {
    for resident in [false, true] {
        for entries in [false, true] {
            for (cause, present, selector, secondary, fields) in [
                (
                    ExceptionCause32::BreakpointInt3,
                    true,
                    0x40,
                    ExceptionCause32::GeneralProtection(SelectorErrorCode32::idt(3, false)),
                    [3, 0, 0x1a, 0, 0, 0],
                ),
                (
                    ExceptionCause32::InvalidOpcode,
                    false,
                    0x40,
                    ExceptionCause32::SegmentNotPresent(SelectorErrorCode32::idt(6, true)),
                    [6, 0, 0x33, 0, 0, 0],
                ),
                (
                    ExceptionCause32::InvalidOpcode,
                    true,
                    0,
                    ExceptionCause32::GeneralProtection(SelectorErrorCode32::null(true)),
                    [3, 0, 1, 0, 0, 4],
                ),
            ] {
                let (mut engine, id) = engine(
                    resident,
                    entries,
                    cause.vector(),
                    15,
                    0,
                    present,
                    selector,
                    0xf33f_ffff,
                );
                let before = engine.arena().to_vec();
                let expected_ram = ram(&engine);
                assert_eq!(
                    invoke(
                        &mut engine,
                        id,
                        cause,
                        FrameInput32 {
                            eip: 0x1001,
                            esp: 0x8000,
                            flags: FLAGS,
                            cs: 0x1b
                        }
                    ),
                    Ok(EventDeliveryResult32::ArchitecturalSecondary(secondary))
                );
                let mut want = before;
                want[HELPER_OFFSET..HELPER_OFFSET + 40].copy_from_slice(&packet(fields));
                assert_eq!(engine.arena(), want);
                assert_eq!(ram(&engine), expected_ram);
            }
        }
    }
}

#[test]
fn fourth_error_slot_fault_has_no_earlier_write_or_version_and_cached_ss_precedes_pages() {
    for resident in [false, true] {
        for entries in [false, true] {
            for (stack, result, fields) in [
                (
                    0xf33f_ffff,
                    EventDeliveryResult32::NeedsPagingAuthority {
                        fault: MemoryFault {
                            address: GuestAddress(0x6ffc),
                            access: Access::Write,
                            reason: FaultReason::Unmapped,
                        },
                        length: 4,
                    },
                    [1, 0, 1, 0x6ffc, 2, 0x204],
                ),
                (
                    0xf270_6fff,
                    EventDeliveryResult32::ArchitecturalSecondary(ExceptionCause32::StackSegment(
                        SelectorErrorCode32::null(true),
                    )),
                    [7, 0, 1, 0, 0, 0x204],
                ),
            ] {
                let (mut engine, id) = engine(resident, entries, 13, 15, 0, true, 0x40, stack);
                let snapshot = engine
                    .memory()
                    .unwrap()
                    .snapshot_code(GuestAddress(0x7000), 4096)
                    .unwrap();
                let before = engine.arena().to_vec();
                let expected_ram = ram(&engine);
                assert_eq!(
                    invoke(
                        &mut engine,
                        id,
                        ExceptionCause32::GeneralProtection(SelectorErrorCode32::null(false)),
                        FrameInput32 {
                            eip: 0x1000,
                            esp: 0x700c,
                            flags: FLAGS,
                            cs: 0x1b
                        }
                    ),
                    Ok(result)
                );
                let mut want = before;
                want[HELPER_OFFSET..HELPER_OFFSET + 40].copy_from_slice(&packet(fields));
                assert_eq!(engine.arena(), want);
                assert_eq!(ram(&engine), expected_ram);
                assert!(
                    engine.memory().unwrap().is_code_current(&snapshot),
                    "failed preflight preserves page version"
                );
            }
        }
    }
}

#[test]
fn completed_and_partial_debug_origins_save_different_rf_without_instruction_retirement() {
    for resident in [false, true] {
        for entries in [false, true] {
            for (debug, eip, flags, saved_flags) in [
                (DebugCause32::Int1, 0x1001, FLAGS, FLAGS),
                (
                    DebugCause32::SingleStep(TrapPoint32::CompletedInstruction),
                    0x1200,
                    FLAGS | 0x100,
                    FLAGS | 0x100,
                ),
                (
                    DebugCause32::SingleStep(TrapPoint32::RepetitionInProgress),
                    0x1000,
                    FLAGS | 0x100,
                    FLAGS | 0x10100,
                ),
            ] {
                let (mut engine, id) = engine(resident, entries, 1, 15, 0, true, 0x40, 0xf33f_ffff);
                let before = engine.arena().to_vec();
                let mut expected_ram = ram(&engine);
                let words = [eip, 0x1b, saved_flags];
                for (index, value) in words.into_iter().enumerate() {
                    expected_ram[1][0xff4 + index * 4..0xff8 + index * 4]
                        .copy_from_slice(&value.to_le_bytes());
                }
                assert_eq!(
                    invoke(
                        &mut engine,
                        id,
                        ExceptionCause32::Debug(debug),
                        FrameInput32 {
                            eip,
                            esp: 0x8000,
                            flags,
                            cs: 0x1b
                        }
                    ),
                    Ok(EventDeliveryResult32::Entered(EnteredEvent32 {
                        target: 0x2345,
                        esp: 0x7ff4,
                        flags: FLAGS,
                        frame_info: 4,
                        code_invalidated: false,
                    }))
                );
                let mut want = before;
                let (sg, sb) = context(0xf33f_ffff, 0x43, true);
                want[SEGMENTS_OFFSET..SEGMENTS_OFFSET + 64].copy_from_slice(&sg);
                want[SEGMENT_BASES_RECORD_OFFSET..SEGMENT_BASES_RECORD_OFFSET + 40]
                    .copy_from_slice(&sb);
                want[HELPER_OFFSET..HELPER_OFFSET + 40]
                    .copy_from_slice(&packet([0, 0x2345, 0, 0, 0, 4]));
                assert_eq!(
                    engine.arena(),
                    want,
                    "core does not manufacture retirement or change unrelated state"
                );
                assert_eq!(ram(&engine), expected_ram);
            }
        }
    }
}

#[test]
fn certified_legacy_paging_metadata_is_explicit_and_rejects_impossible_supervisor_protection() {
    let profile = PageFaultProfile32::legacy32(None);
    let fault = VerifiedPageFault32::new(
        profile,
        GuestAddress(0x6000),
        PagingAccess32::Read,
        PagingDomain32::User,
        LegacyPageViolation32::NotPresent,
    )
    .unwrap();
    assert_eq!(fault.error_code(), 4);
    assert_eq!(fault.linear(), GuestAddress(0x6000));
    assert!(
        VerifiedPageFault32::new(
            profile,
            GuestAddress(0x6000),
            PagingAccess32::Read,
            PagingDomain32::Supervisor,
            LegacyPageViolation32::SupervisorPage
        )
        .is_err()
    );
    assert!(
        VerifiedPageFault32::new(
            profile,
            GuestAddress(0x6000),
            PagingAccess32::Write,
            PagingDomain32::Supervisor,
            LegacyPageViolation32::ReadOnlyPage
        )
        .is_err()
    );
    assert_eq!(
        VerifiedPageFault32::new(
            PageFaultProfile32::legacy32(Some(true)),
            GuestAddress(0x6000),
            PagingAccess32::Write,
            PagingDomain32::Supervisor,
            LegacyPageViolation32::ReadOnlyPage
        )
        .unwrap()
        .error_code(),
        3
    );
}
