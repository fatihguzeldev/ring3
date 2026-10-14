use ring3_engine::{
    abi::{
        arena::{SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET, TRANSFER_OFFSET},
        x86::encode_current_segment_context,
    },
    cpu::x86::SegmentCache,
    memory::GuestAddress,
    process::EngineInstance,
};

fn compile(code: &[u8], d16: bool, resident: bool, entries: bool, length: usize) {
    let mut engine = EngineInstance::new(1, 0x494e_544e).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + code.len()]
        .copy_from_slice(code);
    engine.upload(0x1100, code.len() as u32).unwrap();
    if d16 {
        let mut context = engine.segment_context().unwrap();
        context.caches[1] = SegmentCache::from_bits(0xf1bf_ffff).unwrap();
        let mut segments = [0; 64];
        let mut bases = [0; 40];
        encode_current_segment_context(&context, &mut segments, &mut bases).unwrap();
        let arena = engine.arena_mut().unwrap();
        arena[SEGMENTS_OFFSET..SEGMENTS_OFFSET + segments.len()].copy_from_slice(&segments);
        arena[SEGMENT_BASES_RECORD_OFFSET..SEGMENT_BASES_RECORD_OFFSET + bases.len()]
            .copy_from_slice(&bases);
    }
    let arena = engine.arena_mut().unwrap();
    arena[TRANSFER_OFFSET..TRANSFER_OFFSET + 4].copy_from_slice(&0x1100_u32.to_le_bytes());
    if !entries {
        arena[TRANSFER_OFFSET + 4..TRANSFER_OFFSET + 8]
            .copy_from_slice(&(length as u32).to_le_bytes());
    }
    let before = engine.arena().to_vec();
    let mut ram = vec![0; 4096];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x1000), &mut ram)
        .unwrap();
    let id = match (resident, entries) {
        (false, false) => engine.compile(1).map(|_| None),
        (false, true) => engine.compile_entries(1, 0).map(|_| None),
        (true, false) => engine.compile_resident(1).map(|id| Some(id.get())),
        (true, true) => engine
            .compile_resident_entries(1, 0)
            .map(|id| Some(id.get())),
    }
    .expect("application INT compiles without hardware interrupt tables");
    let module = id.map_or_else(
        || engine.artifact_bytes().unwrap(),
        |id| engine.resident_bytes(id).unwrap(),
    );
    assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
    assert_eq!(
        engine.arena(),
        before,
        "compilation publishes no CPU or helper state"
    );
    let mut after = vec![0; 4096];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x1000), &mut after)
        .unwrap();
    assert_eq!(after, ram);
}

#[test]
fn software_interrupts_compile_in_both_defaults_without_hardware_table_configuration() {
    for code in [&[0xcd, 0x80][..], &[0x66, 0xcd, 0x80]] {
        for d16 in [false, true] {
            for (resident, entries) in [(false, false), (false, true), (true, false), (true, true)]
            {
                compile(code, d16, resident, entries, code.len());
            }
        }
    }
}

#[test]
fn entry_capture_stops_at_each_int_terminator_before_an_invalid_tail() {
    for code in [
        &[0xcd, 0x80, 0xf0, 0x90][..],
        &[0x66, 0xcd, 0x80, 0xf0, 0x90],
    ] {
        for d16 in [false, true] {
            for resident in [false, true] {
                compile(code, d16, resident, true, code.len() - 2);
            }
        }
    }
}
