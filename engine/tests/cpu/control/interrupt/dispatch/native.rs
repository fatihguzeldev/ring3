use ring3_engine::{
    abi::arena::{HELPER_OFFSET, SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET, TRANSFER_OFFSET},
    cpu::x86::{InterruptDescriptorTable32, SegmentDescriptorRecord},
    memory::GuestAddress,
    process::{EngineInstance, HostError, StoreCompletion},
};
const KEY: u64 = 0x494e544e;
const VECTOR: u32 = 0x80;
fn engine(
    kind: u32,
    target_high: u32,
    resident: bool,
    entries: bool,
) -> (EngineInstance, Option<u64>) {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    let mut records = Box::new([SegmentDescriptorRecord::from_words(0, 0); 256]);
    records[0x80] = SegmentDescriptorRecord::from_words(
        if kind == 5 { 0x00400000 } else { 0x00402345 },
        0xe000 | (kind << 8),
    );
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
    esp: u32,
) -> Result<StoreCompletion, HostError> {
    match id {
        Some(id) => engine.interrupt_gate_resident(KEY, id, VECTOR, 0xabcd1234, esp, 0x254fd7),
        None => engine.interrupt_gate(VECTOR, 0xabcd1234, esp, 0x254fd7),
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
fn packet(fields: [u32; 6]) -> Vec<u8> {
    let mut bytes = b"R3MH\x0b\0\x01\0\x28\0\0\0\0\0\0\0".to_vec();
    for value in fields {
        bytes.extend(value.to_le_bytes());
    }
    bytes
}

#[test]
fn configured_same_cpl_gate_frames_have_gate_selected_width_saved_rf_zero_and_no_live_register_commit()
 {
    for resident in [false, true] {
        for entries in [false, true] {
            for (kind, frame, info) in [
                (7, vec![0x34, 0x12, 0x1b, 0, 0xd7, 0x4f], 2),
                (
                    14,
                    vec![0x34, 0x12, 0xcd, 0xab, 0x1b, 0, 0, 0, 0xd7, 0x4f, 0x24, 0],
                    0x104,
                ),
            ] {
                let (mut engine, id) = engine(kind, 0x00cffa00, resident, entries);
                let before = engine.arena().to_vec();
                let mut expected_ram = ram(&engine);
                assert_eq!(
                    invoke(&mut engine, id, 0x7801),
                    Ok(StoreCompletion::Complete)
                );
                let start = 0x801 - frame.len();
                expected_ram[1][start..start + frame.len()].copy_from_slice(&frame);
                assert_eq!(
                    ram(&engine),
                    expected_ram,
                    "all mapped RAM changes only the complete literal frame"
                );
                assert_eq!(
                    &engine.arena()[HELPER_OFFSET..HELPER_OFFSET + 40],
                    packet([0, 0x2345, 0, 0, 0, info])
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
                let mut expected = before;
                for (offset, length) in [
                    (HELPER_OFFSET, 40),
                    (SEGMENTS_OFFSET, 64),
                    (SEGMENT_BASES_RECORD_OFFSET, 40),
                ] {
                    expected[offset..offset + length]
                        .copy_from_slice(&engine.arena()[offset..offset + length]);
                }
                assert_eq!(
                    engine.arena(),
                    expected,
                    "helper does not publish CPU EIP/ESP/flags or unrelated state"
                );
            }
        }
    }
}

#[test]
fn late_frame_fault_and_outer_admitted_task_or_privilege_boundaries_are_packet_only_and_guarded() {
    for resident in [false, true] {
        for entries in [false, true] {
            for (kind, target_high, esp, fields) in [
                (14, 0x00cffa00, 0x7008, [1, 0, 1, 0x6ffc, 2, 0x104]),
                (5, 0x00cffa00, 0x7800, [8, 0, 4, 0, 0, 0]),
                (14, 0x00cf9a00, 0x7800, [8, 0, 4, 0, 0, 0]),
            ] {
                let (mut engine, id) = engine(kind, target_high, resident, entries);
                let before = engine.arena().to_vec();
                let expected_ram = ram(&engine);
                assert_eq!(invoke(&mut engine, id, esp), Ok(StoreCompletion::Complete));
                let mut expected = before.clone();
                expected[HELPER_OFFSET..HELPER_OFFSET + 40].copy_from_slice(&packet(fields));
                assert_eq!(
                    engine.arena(),
                    expected,
                    "exact typed packet is the only publication"
                );
                assert_eq!(
                    ram(&engine),
                    expected_ram,
                    "no partial stack frame, including successful earlier page probes"
                );
                if let Some(id) = id {
                    let before = engine.arena().to_vec();
                    assert!(
                        engine
                            .interrupt_gate_resident(KEY ^ 1, id, 256, 0, 0, 0)
                            .is_err()
                    );
                    assert_eq!(engine.arena(), before);
                    assert_eq!(ram(&engine), expected_ram);
                }
            }
        }
    }
}
