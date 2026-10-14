use ring3_engine::{
    abi::{
        arena::{EXIT_OFFSET, SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET, TRANSFER_OFFSET},
        x86::{
            EXIT_SIZE, SEGMENT_BASES_RECORD_SIZE, SEGMENTS_SIZE, STATE_SIZE,
            encode_current_segment_context, encode_exit, encode_state_v3,
        },
    },
    cpu::{
        ExecutionExit, ExitReason,
        x86::{
            SegmentCache, State32,
            decode::{DecodeError, decode_one},
        },
    },
    memory::GuestAddress,
    process::EngineInstance,
};
const KEY: u64 = 0x5341_4c43_4346_414c;
fn upload(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
}
fn stopped(bytes: &[u8], default32: bool) -> EngineInstance {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    for page in [0x1000, 0x3000] {
        engine.map(page, 1, 7).unwrap();
    }
    upload(&mut engine, 0x1000, &[0xe9, 0xfb, 0x1f, 0, 0]);
    upload(&mut engine, 0x3000, bytes);
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0x1000_u32.to_le_bytes(), 5_u32.to_le_bytes()].concat());
    engine.compile(1).unwrap();
    if !default32 {
        let mut context = engine.segment_context().unwrap();
        context.caches[1] = SegmentCache::from_bits(context.caches[1].bits() & !(1 << 25)).unwrap();
        let mut segments = [0; SEGMENTS_SIZE];
        let mut bases = [0; SEGMENT_BASES_RECORD_SIZE];
        encode_current_segment_context(&context, &mut segments, &mut bases).unwrap();
        let arena = engine.arena_mut().unwrap();
        arena[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE].copy_from_slice(&segments);
        arena[SEGMENT_BASES_RECORD_OFFSET..SEGMENT_BASES_RECORD_OFFSET + SEGMENT_BASES_RECORD_SIZE]
            .copy_from_slice(&bases);
    }
    let arena = engine.arena_mut().unwrap();
    encode_state_v3(
        &State32 {
            registers: [0xa1b2_c3d4; 8],
            eip: 0x3000,
            eflags: 0x10dd7,
        },
        &mut arena[..STATE_SIZE],
    )
    .unwrap();
    arena[12..16].copy_from_slice(&1_u32.to_le_bytes());
    encode_exit(
        &ExecutionExit {
            retired: 1,
            reason: ExitReason::NeedCode,
        },
        &mut arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    engine
}
fn ram(engine: &EngineInstance) -> [Vec<u8>; 2] {
    [0x1000, 0x3000].map(|page| {
        let mut bytes = vec![0; 4096];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(page), &mut bytes)
            .unwrap();
        bytes
    })
}
fn prepares(bytes: &[u8], invalid: bool) {
    for default32 in [false, true] {
        for resident in [false, true] {
            let mut engine = stopped(bytes, default32);
            let diagnostic = decode_one(engine.memory().unwrap(), GuestAddress(0x3000));
            if invalid {
                assert_eq!(diagnostic.unwrap_err(), DecodeError::InvalidEncoding);
            } else {
                assert!(
                    diagnostic.is_ok(),
                    "strict SALC/control admission: {bytes:02x?}"
                );
            }
            let before = engine.arena().to_vec();
            let before_ram = ram(&engine);
            if resident {
                let id = engine
                    .compile_stopped_resident_entry(KEY)
                    .expect("resident instruction preparation");
                assert_eq!(
                    &engine.resident_bytes(id.get()).unwrap()[..8],
                    b"\0asm\x01\0\0\0"
                );
                assert_eq!(engine.generation(), 1);
            } else {
                assert_eq!(engine.compile_stopped_entry(KEY, 1), Ok(2));
                assert_eq!(&engine.artifact_bytes().unwrap()[..8], b"\0asm\x01\0\0\0");
            }
            assert_eq!(engine.arena(), before);
            assert_eq!(ram(&engine), before_ram);
        }
    }
}
#[test]
fn salc_prepares_in_both_code_defaults_without_cpu_or_memory_publication() {
    prepares(&[0xd6, 0xeb, 0xfe], false);
}
#[test]
fn existing_carry_condition_and_invalid_lock_controls_keep_their_owners() {
    prepares(&[0x0f, 0x92, 0xc0, 0xeb, 0xfe], false);
    prepares(&[0xf0, 0xd6], true);
}
