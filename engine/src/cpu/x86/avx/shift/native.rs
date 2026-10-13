use super::*;
use crate::cpu::x86::simd::{PackedKind as P, ShiftOperation};
fn r(i: u8) -> VectorRegister {
    VectorRegister::new(i).unwrap()
}
fn lane(kind: P, width: u8, d: u8, a: u8, count: ShiftCount) -> ShiftInstruction {
    ShiftInstruction::new(
        ShiftOperation::lanes(kind, width).unwrap(),
        r(d),
        r(a),
        count,
    )
    .unwrap()
}
fn state() -> VectorState {
    let mut s = VectorState::default();
    for i in 0..8 {
        s.simd.xmm[i] = [0x80 + i as u8; 16];
        s.upper[i] = [0xb0 + i as u8; 16];
    }
    s.simd.mxcsr = 0xe5af;
    s
}
#[test]
fn full_low64_count_never_uses_wasm_modulo_or_high64() {
    let mut s = state();
    s.simd.xmm[1] = [0xff; 16];
    s.simd.xmm[2] = [0xff; 16];
    s.simd.xmm[2][..8].copy_from_slice(&1u64.to_le_bytes());
    s.execute_shift(
        lane(P::ShiftRight, 8, 0, 1, ShiftCount::Register(r(2))),
        &[],
    )
    .unwrap();
    assert_eq!(
        s.simd.xmm[0],
        [
            255, 255, 255, 255, 255, 255, 255, 127, 255, 255, 255, 255, 255, 255, 255, 127
        ]
    );
    s.simd.xmm[2][..8].copy_from_slice(&(1u64 << 32).to_le_bytes());
    s.execute_shift(lane(P::ShiftLeft, 4, 0, 1, ShiftCount::Register(r(2))), &[])
        .unwrap();
    assert_eq!(s.simd.xmm[0], [0; 16]);
    s.execute_shift(
        lane(P::ShiftRightSigned, 2, 0, 1, ShiftCount::Register(r(2))),
        &[],
    )
    .unwrap();
    assert_eq!(s.simd.xmm[0], [255; 16]);
    assert_eq!(s.upper[0], [0; 16]);
}
#[test]
fn immediate_counts_and_byte_shifts_have_exact_boundaries() {
    for n in [0, 15, 16, 17, 255] {
        let mut s = state();
        s.simd.xmm[1] = std::array::from_fn(|i| i as u8 + 1);
        s.execute_shift(
            ShiftInstruction::new(
                ShiftOperation::bytes(true),
                r(0),
                r(1),
                ShiftCount::Immediate(n),
            )
            .unwrap(),
            &[],
        )
        .unwrap();
        let mut expected = [0; 16];
        if n < 16 {
            expected[usize::from(n)..]
                .copy_from_slice(&(1u8..=16).collect::<Vec<_>>()[..16 - usize::from(n)]);
        }
        assert_eq!(s.simd.xmm[0], expected);
        s.execute_shift(
            lane(P::ShiftRightSigned, 4, 0, 1, ShiftCount::Immediate(255)),
            &[],
        )
        .unwrap();
        assert_eq!(s.simd.xmm[0], [0; 16]);
    }
}
#[test]
fn aliases_and_exact_memory_counts_preserve_unrelated_state() {
    for (d, a, b) in [(0, 1, 2), (0, 0, 2), (0, 1, 0), (0, 1, 1), (0, 0, 0)] {
        let mut s = state();
        s.simd.xmm[b as usize][..8].copy_from_slice(&17u64.to_le_bytes());
        let before = s;
        s.execute_shift(
            lane(P::ShiftRight, 2, d, a, ShiftCount::Register(r(b))),
            &[],
        )
        .unwrap();
        let mut expected = before;
        expected.simd.xmm[d as usize] = [0; 16];
        expected.upper[d as usize] = [0; 16];
        assert_eq!(s, expected);
    }
    let mut s = state();
    let before = s;
    let i = lane(P::ShiftLeft, 8, 0, 1, ShiftCount::Memory);
    assert_eq!(
        i.memory(),
        Some(MemoryRequirement {
            access: Access::Read,
            bytes: 16,
            alignment: 1
        })
    );
    assert_eq!(s.execute_shift(i, &[0; 8]), Err(ShiftError::OperandLength));
    assert_eq!(s, before);
    s.simd.mxcsr = 0x10000;
    let before = s;
    assert_eq!(s.execute_shift(i, &[0; 16]), Err(ShiftError::InvalidState));
    assert_eq!(s, before);
}
#[test]
fn closed_eighteen_forms_reject_avx2_and_byte_count_sources() {
    let mut forms = 2;
    for kind in [P::ShiftLeft, P::ShiftRight, P::ShiftRightSigned] {
        for width in [1, 2, 4, 8] {
            if ShiftOperation::lanes(kind, width).is_some() {
                forms += 2;
            }
        }
    }
    assert_eq!(forms, 18);
    assert!(ShiftOperation::lanes(P::ShiftLeft, 1).is_none());
    assert!(ShiftOperation::lanes(P::ShiftRightSigned, 8).is_none());
    assert!(ShiftOperation::lanes(P::Add, 4).is_none());
    assert!(
        ShiftInstruction::new(ShiftOperation::bytes(false), r(0), r(1), ShiftCount::Memory)
            .is_none()
    );
    assert!(
        ShiftInstruction::new(
            ShiftOperation::bytes(false),
            r(0),
            r(1),
            ShiftCount::Register(r(2))
        )
        .is_none()
    );
}
