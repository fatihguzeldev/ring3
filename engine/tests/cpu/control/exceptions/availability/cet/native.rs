use ring3_engine::{
    abi::{
        arena::{EXIT_OFFSET, TRANSFER_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, encode_exit, encode_state_v3},
    },
    cpu::{
        ExecutionExit, ExitReason,
        x86::{
            State32,
            decode::{DecodeError, decode_one},
        },
    },
    memory::GuestAddress,
    process::EngineInstance,
};

const KEY: u64 = 0x504b_5255_4345_5430;

fn upload(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
}

fn stopped(bytes: &[u8], pc: u32, registers: [u32; 8]) -> EngineInstance {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    for page in [0x1000, 0x3000] {
        engine.map(page, 1, 7).unwrap();
    }
    let mut jump = vec![0xe9];
    jump.extend((pc - 0x1005).to_le_bytes());
    upload(&mut engine, 0x1000, &jump);
    upload(&mut engine, pc, bytes);
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0x1000_u32.to_le_bytes(), 5_u32.to_le_bytes()].concat());
    engine.compile(1).unwrap();
    let arena = engine.arena_mut().unwrap();
    encode_state_v3(
        &State32 {
            registers,
            eip: pc,
            eflags: 0x10dd7,
        },
        &mut arena[..STATE_SIZE],
    )
    .unwrap();
    // existing protected32 stop authority: RF+TF and an inherited SS shadow.
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

fn prepares(bytes: &[u8], registers: [u32; 8]) {
    for resident in [false, true] {
        for pc in [0x3000, 0x4000 - bytes.len() as u32] {
            let mut engine = stopped(bytes, pc, registers);
            assert!(
                matches!(
                    decode_one(engine.memory().unwrap(), GuestAddress(pc)),
                    Err(DecodeError::Unsupported(_))
                ),
                "strict diagnostic decoder remains strict: {bytes:02x?}"
            );
            let before = engine.arena().to_vec();
            let before_ram = ram(&engine);
            if resident {
                let id = engine
                    .compile_stopped_resident_entry(KEY)
                    .expect("disabled state must prepare a real resident UD instruction");
                assert_ne!(id.get(), 0);
                assert_eq!(
                    &engine.resident_bytes(id.get()).unwrap()[..8],
                    b"\0asm\x01\0\0\0"
                );
                assert_eq!(engine.generation(), 1);
            } else {
                assert_eq!(
                    engine.compile_stopped_entry(KEY, 1),
                    Ok(2),
                    "disabled state must prepare UD: {bytes:02x?}"
                );
                assert_eq!(&engine.artifact_bytes().unwrap()[..8], b"\0asm\x01\0\0\0");
            }
            assert_eq!(
                engine.arena(),
                before,
                "preparation must not execute or publish CPU/helper state"
            );
            assert_eq!(ram(&engine), before_ram, "preparation changes no guest RAM");
        }
    }
}

#[test]
fn disabled_cet_prepares_rstorssp_before_shadow_stack_memory_or_privilege() {
    prepares(&[0xf3, 0x0f, 0x01, 0x28], [u32::MAX; 8]);
}

#[test]
fn disabled_cet_prepares_saveprevssp_before_shadow_stack_memory_or_privilege() {
    prepares(&[0xf3, 0x0f, 0x01, 0xea], [u32::MAX; 8]);
}

#[test]
fn disabled_cet_prepares_setssbsy_before_shadow_stack_memory_or_privilege() {
    prepares(&[0xf3, 0x0f, 0x01, 0xe8], [u32::MAX; 8]);
}

#[test]
fn disabled_cet_prepares_clrssbsy_before_shadow_stack_memory_or_privilege() {
    prepares(&[0xf3, 0x0f, 0xae, 0x30], [u32::MAX; 8]);
}

#[test]
fn disabled_cet_prepares_wrssd_before_shadow_stack_memory_or_privilege() {
    prepares(&[0x0f, 0x38, 0xf6, 0x00], [u32::MAX; 8]);
}

#[test]
fn disabled_cet_prepares_wrussd_before_shadow_stack_memory_or_privilege() {
    prepares(&[0x66, 0x0f, 0x38, 0xf5, 0x00], [u32::MAX; 8]);
}
