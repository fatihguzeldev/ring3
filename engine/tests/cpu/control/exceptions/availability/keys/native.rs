use super::stopped_support::{KEY, stopped};
use ring3_engine::{
    cpu::x86::{decode::decode_one, ir::Operation},
    memory::GuestAddress,
};

fn ram(engine: &ring3_engine::process::EngineInstance) -> Vec<u8> {
    let mut bytes = vec![0; 8192];
    for (index, page) in [0x1000, 0x3000].into_iter().enumerate() {
        engine
            .memory()
            .unwrap()
            .read(
                GuestAddress(page),
                &mut bytes[index * 4096..(index + 1) * 4096],
            )
            .unwrap();
    }
    bytes
}

fn capture_absent_fault(bytes: &[u8]) {
    for resident in [false, true] {
        for pc in [0x3000, 0x4000 - bytes.len() as u32] {
            let mut engine = stopped(bytes, pc);
            assert!(decode_one(engine.memory().unwrap(), GuestAddress(pc)).is_err());
            let keep = engine.compile_resident(1).unwrap();
            let prior = engine.artifact_bytes().unwrap().to_vec();
            let prior_resident = engine.resident_bytes(keep.get()).unwrap().to_vec();
            let arena = engine.arena().to_vec();
            let memory = ram(&engine);
            if resident {
                let result = engine.compile_stopped_resident_entry(KEY);
                assert!(result.is_ok(), "{bytes:02x?}: {result:?}");
                assert_eq!(
                    &engine.resident_bytes(result.unwrap().get()).unwrap()[..8],
                    b"\0asm\x01\0\0\0"
                );
                assert_eq!(engine.generation(), 1);
                assert_eq!(engine.artifact_bytes().unwrap(), prior);
            } else {
                assert_eq!(engine.compile_stopped_entry(KEY, 1), Ok(2), "{bytes:02x?}");
                assert_eq!(engine.generation(), 2);
                assert_eq!(&engine.artifact_bytes().unwrap()[..8], b"\0asm\x01\0\0\0");
            }
            assert_eq!(engine.resident_bytes(keep.get()).unwrap(), prior_resident);
            assert_eq!(engine.arena(), arena);
            assert_eq!(ram(&engine), memory);
        }
    }
}

#[test]
fn absent_aesencwide128kl_m384_prepares_ud() {
    capture_absent_fault(&[0xf3, 0x0f, 0x38, 0xd8, 0x00]);
}
#[test]
fn absent_aesdecwide128kl_m384_prepares_ud() {
    capture_absent_fault(&[0xf3, 0x0f, 0x38, 0xd8, 0x08]);
}
#[test]
fn absent_aesencwide256kl_m512_prepares_ud() {
    capture_absent_fault(&[0xf3, 0x0f, 0x38, 0xd8, 0x10]);
}
#[test]
fn absent_aesdecwide256kl_m512_prepares_ud() {
    capture_absent_fault(&[0xf3, 0x0f, 0x38, 0xd8, 0x18]);
}
#[test]
fn absent_aesenc128kl_xmm_m384_prepares_ud() {
    capture_absent_fault(&[0xf3, 0x0f, 0x38, 0xdc, 0x00]);
}
#[test]
fn absent_aesdec128kl_xmm_m384_prepares_ud() {
    capture_absent_fault(&[0xf3, 0x0f, 0x38, 0xdd, 0x00]);
}
#[test]
fn absent_aesenc256kl_xmm_m512_prepares_ud() {
    capture_absent_fault(&[0xf3, 0x0f, 0x38, 0xde, 0x00]);
}
#[test]
fn absent_aesdec256kl_xmm_m512_prepares_ud() {
    capture_absent_fault(&[0xf3, 0x0f, 0x38, 0xdf, 0x00]);
}
#[test]
fn absent_encodekey128_r32_r32_prepares_ud() {
    capture_absent_fault(&[0xf3, 0x0f, 0x38, 0xfa, 0xc1]);
}
#[test]
fn absent_encodekey256_r32_r32_prepares_ud() {
    capture_absent_fault(&[0xf3, 0x0f, 0x38, 0xfb, 0xc1]);
}
#[test]
fn absent_loadiwkey_xmm_xmm_prepares_ud() {
    capture_absent_fault(&[0xf3, 0x0f, 0x38, 0xdc, 0xc1]);
}
#[test]
fn vex128_aes_selected_absence_and_neutral_neighbor_remain_distinct() {
    for opcode in [0xdc, 0xdd, 0xde, 0xdf] {
        let bytes = [0xc4, 0xe2, 0x79, opcode, 0xc1];
        let engine = stopped(&bytes, 0x3000);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(0x3000)).unwrap();
        assert!(matches!(decoded.operation(), Operation::Avx(_)));
        assert_eq!(decoded.length(), 5);
    }
    for bytes in [&[0xcd, 0x80][..], &[0xc4, 0xe3, 0x7d, 0x44, 0xc1, 0]] {
        let mut engine = stopped(bytes, 0x3000);
        let arena = engine.arena().to_vec();
        assert!(decode_one(engine.memory().unwrap(), GuestAddress(0x3000)).is_err());
        let result = engine.compile_stopped_entry(KEY, 1);
        if bytes == [0xc4, 0xe3, 0x7d, 0x44, 0xc1, 0] {
            assert_eq!(result, Ok(2));
            assert_eq!(engine.generation(), 2);
        } else {
            assert!(result.is_err());
            assert_eq!(engine.generation(), 1);
        }
        assert_eq!(engine.arena(), arena);
    }
}
