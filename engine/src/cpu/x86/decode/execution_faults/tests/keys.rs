use crate::{
    cpu::x86::{
        ControlOperation,
        capabilities::CPU_ID_LEAVES,
        decode::{DecodeError, decode_for_execution, decode_one},
        ir::Operation,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
use iced_x86::{Code, Decoder, DecoderOptions};
fn executable(bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    let page = PageRange::new(GuestAddress(0x1000), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    memory
}
#[test]
fn key_locker_absence_is_ud_before_key_control_privilege_or_memory_authority() {
    let leaf = CPU_ID_LEAVES
        .iter()
        .find(|leaf| leaf.function == 7 && leaf.subfunction == Some(0))
        .unwrap();
    assert_eq!(leaf.registers[2] & (1 << 23), 0);
    let rows: [(Code, &[u8]); 11] = [
        (Code::Aesencwide128kl_m384, &[0xf3, 0x0f, 0x38, 0xd8, 0x00]),
        (Code::Aesdecwide128kl_m384, &[0xf3, 0x0f, 0x38, 0xd8, 0x08]),
        (Code::Aesencwide256kl_m512, &[0xf3, 0x0f, 0x38, 0xd8, 0x10]),
        (Code::Aesdecwide256kl_m512, &[0xf3, 0x0f, 0x38, 0xd8, 0x18]),
        (Code::Aesenc128kl_xmm_m384, &[0xf3, 0x0f, 0x38, 0xdc, 0x00]),
        (Code::Aesdec128kl_xmm_m384, &[0xf3, 0x0f, 0x38, 0xdd, 0x00]),
        (Code::Aesenc256kl_xmm_m512, &[0xf3, 0x0f, 0x38, 0xde, 0x00]),
        (Code::Aesdec256kl_xmm_m512, &[0xf3, 0x0f, 0x38, 0xdf, 0x00]),
        (Code::Encodekey128_r32_r32, &[0xf3, 0x0f, 0x38, 0xfa, 0xc1]),
        (Code::Encodekey256_r32_r32, &[0xf3, 0x0f, 0x38, 0xfb, 0xc1]),
        (Code::Loadiwkey_xmm_xmm, &[0xf3, 0x0f, 0x38, 0xdc, 0xc1]),
    ];
    for (code, bytes) in rows {
        let raw = Decoder::new(32, bytes, DecoderOptions::NONE).decode();
        assert_eq!(raw.code(), code);
        assert!(!raw.has_rep_prefix() && !raw.has_repne_prefix());
        let memory = executable(bytes);
        assert!(matches!(
            decode_one(&memory, GuestAddress(0x1000)),
            Err(DecodeError::Unsupported(_))
        ));
        let decoded = decode_for_execution(&memory, GuestAddress(0x1000)).unwrap();
        assert_eq!(
            decoded.operation(),
            &Operation::Control(ControlOperation::InvalidOpcode)
        );
        assert_eq!(decoded.segment_accesses(), 0);
        assert_eq!(usize::from(decoded.length()), bytes.len());
        assert!(memory.is_code_current(decoded.code_snapshot()));
    }
}
#[test]
fn ordinary_narrow_aes_and_unselected_neutral_refusal_are_preserved() {
    for opcode in [0xdc, 0xdd, 0xde, 0xdf] {
        for prefix in [vec![0x66, 0x0f, 0x38], vec![0xc4, 0xe2, 0x79]] {
            let mut bytes = prefix;
            bytes.extend([opcode, 0xc1]);
            let memory = executable(&bytes);
            let strict = decode_one(&memory, GuestAddress(0x1000)).unwrap();
            assert!(matches!(
                strict.operation(),
                Operation::Avx(_) | Operation::Simd(_)
            ));
            assert_eq!(
                decode_for_execution(&memory, GuestAddress(0x1000))
                    .unwrap()
                    .operation(),
                strict.operation()
            );
        }
    }
    let memory = executable(&[0xcd, 0x80]);
    let strict = decode_one(&memory, GuestAddress(0x1000)).unwrap_err();
    assert!(matches!(strict, DecodeError::Unsupported(_)));
    assert_eq!(
        decode_for_execution(&memory, GuestAddress(0x1000)).unwrap_err(),
        strict
    );
}
