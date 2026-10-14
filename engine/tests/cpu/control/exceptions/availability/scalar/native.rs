use super::support_fetch as fetch;
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
    memory::{FaultReason, GuestAddress},
    process::EngineInstance,
};

const KEY: u64 = 0x504b_5255_4345_5430;

fn upload(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
}

fn stopped(bytes: &[u8], pc: u32, registers: [u32; 8]) -> EngineInstance {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
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
    // Existing protected32 stop authority: RF+TF and an inherited SS shadow.
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

fn ram(engine: &EngineInstance) -> Vec<(u32, Vec<u8>)> {
    let mut pages = Vec::new();
    for page in [0x1000, 0x3000, 0x4000] {
        let mut bytes = vec![0; 4096];
        let result = engine
            .memory()
            .unwrap()
            .read(GuestAddress(page), &mut bytes);
        if page != 0x4000 {
            result.unwrap();
            pages.push((page, bytes));
        } else if result.is_ok() {
            pages.push((page, bytes));
        }
    }
    pages
}

fn prepares(bytes: &[u8], nop: bool) {
    for resident in [false, true] {
        let mut code = bytes.to_vec();
        if nop {
            code.extend([0xeb, 0xfe]);
        }
        let mut engine = stopped(&code, 0x3000, [u32::MAX; 8]);
        let diagnostic = decode_one(engine.memory().unwrap(), GuestAddress(0x3000));
        if nop {
            assert!(diagnostic.is_ok());
        } else {
            assert!(matches!(
                diagnostic,
                Err(DecodeError::Unsupported(_) | DecodeError::InvalidEncoding)
            ));
        }
        let before = engine.arena().to_vec();
        let before_ram = ram(&engine);
        if resident {
            let id = engine
                .compile_stopped_resident_entry(KEY)
                .expect("qualified instruction resident capture");
            assert_eq!(
                &engine.resident_bytes(id.get()).unwrap()[..8],
                b"\0asm\x01\0\0\0"
            );
            assert_eq!(engine.generation(), 1);
        } else {
            assert_eq!(engine.compile_stopped_entry(KEY, 1), Ok(2), "{bytes:02x?}");
            assert_eq!(&engine.artifact_bytes().unwrap()[..8], b"\0asm\x01\0\0\0");
        }
        assert_eq!(
            engine.arena(),
            before,
            "capture does not execute the instruction"
        );
        assert_eq!(ram(&engine), before_ram, "capture changes no guest RAM");
    }
}

#[test]
fn exact_tbm_codes_prepare_ud_before_operands() {
    for bytes in [
        &[0x8f, 0xe9, 0x78, 0x01, 0xc8][..],
        &[0x8f, 0xe9, 0x78, 0x01, 0xd0],
        &[0x8f, 0xe9, 0x78, 0x01, 0xd8],
        &[0x8f, 0xe9, 0x78, 0x01, 0xe0],
        &[0x8f, 0xe9, 0x78, 0x01, 0xe8],
        &[0x8f, 0xe9, 0x78, 0x01, 0xf0],
        &[0x8f, 0xe9, 0x78, 0x01, 0xf8],
        &[0x8f, 0xe9, 0x78, 0x02, 0xc8],
        &[0x8f, 0xe9, 0x78, 0x02, 0xf0],
        &[0x8f, 0xea, 0x78, 0x10, 0xc1, 0, 0, 0, 0],
    ] {
        prepares(bytes, false);
    }
}

#[test]
fn monitorx_mwaitx_clzero_and_rdpru_prepare_ud_without_backend_state() {
    for bytes in [
        &[0x0f, 0x01, 0xfa][..],
        &[0x67, 0x0f, 0x01, 0xfa],
        &[0x0f, 0x01, 0xfb],
        &[0x0f, 0x01, 0xfc],
        &[0x67, 0x0f, 0x01, 0xfc],
        &[0x0f, 0x01, 0xfd],
    ] {
        prepares(bytes, false);
    }
}

#[test]
fn clac_and_stac_prepare_cpl3_ud_without_changing_ac() {
    for bytes in [&[0x0f, 0x01, 0xca][..], &[0x0f, 0x01, 0xcb]] {
        prepares(bytes, false);
    }
}

#[test]
fn unadvertised_prefetchit_exact_memory_forms_prepare_nop() {
    for bytes in [&[0x0f, 0x18, 0x30][..], &[0x0f, 0x18, 0x38]] {
        prepares(bytes, true);
    }
}

#[test]
fn complete_executable_source_precedes_qualified_ud_and_nop_capture() {
    for bytes in [
        &[0x8f, 0xea, 0x78, 0x10, 0xc1, 0, 0, 0, 0][..],
        &[0x0f, 0x01, 0xfa],
        &[0x0f, 0x18, 0x30],
    ] {
        let pc = 0x4000 - bytes.len() as u32 + 1;
        let mut engine = stopped(&bytes[..bytes.len() - 1], pc, [u32::MAX; 8]);
        for readonly in [false, true] {
            if readonly {
                engine.map(0x4000, 1, 7).unwrap();
                let mut tail = vec![*bytes.last().unwrap()];
                tail.extend([0xeb, 0xfe]);
                upload(&mut engine, 0x4000, &tail);
                engine.protect(0x4000, 1, 1).unwrap();
            }
            let before = engine.arena().to_vec();
            let before_ram = ram(&engine);
            fetch::assert_error(
                decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap_err(),
                pc,
                0x4000,
                if readonly {
                    FaultReason::Permission
                } else {
                    FaultReason::Unmapped
                },
                bytes.len() as u32,
            );
            let id = engine.compile_stopped_resident_entry(KEY).unwrap();
            assert_eq!(
                &engine.resident_bytes(id.get()).unwrap()[..8],
                b"\0asm\x01\0\0\0"
            );
            assert_eq!(engine.generation(), 1);
            assert_eq!(engine.arena(), before);
            assert_eq!(ram(&engine), before_ram);
            engine
                .discard_unacknowledged_resident(KEY, id.get())
                .unwrap();
            assert_eq!(engine.arena(), before);
            assert_eq!(ram(&engine), before_ram);
        }
        engine.protect(0x4000, 1, 7).unwrap();
        let before = engine.arena().to_vec();
        let before_ram = ram(&engine);
        assert_eq!(engine.compile_stopped_entry(KEY, 1), Ok(2));
        assert_eq!(engine.arena(), before);
        assert_eq!(ram(&engine), before_ram);
    }
}

#[test]
fn prefix_boundary_and_existing_prefetch_control_remain_distinct() {
    for bytes in [&[0x66, 0x66, 0x0f, 0x01, 0xfc][..], &[0x0f, 0x18, 0xf0]] {
        let mut engine = stopped(bytes, 0x3000, [u32::MAX; 8]);
        let before = engine.arena().to_vec();
        let before_ram = ram(&engine);
        assert!(engine.compile_stopped_entry(KEY, 1).is_err());
        assert_eq!(engine.arena(), before);
        assert_eq!(ram(&engine), before_ram);
    }
    prepares(&[0xf0, 0x0f, 0x18, 0x30], false);
    prepares(&[0x0f, 0x0d, 0x00], true);
}
