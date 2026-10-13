use super::*;
use crate::cpu::x86::simd::{MultiplyOperation, PackedKind as P};
fn r(i: u8) -> VectorRegister {
    VectorRegister::new(i).unwrap()
}
fn op(k: P, w: u8, imm: u8) -> MultiplyOperation {
    MultiplyOperation::new(k, w, imm).unwrap()
}
fn i(op: MultiplyOperation, d: u8, a: u8, b: MoveSource) -> MultiplyInstruction {
    MultiplyInstruction::new(op, r(d), (!op.unary()).then_some(r(a)), b).unwrap()
}
fn state() -> VectorState {
    let mut s = VectorState::default();
    for r in 0..8 {
        s.simd.xmm[r] = std::array::from_fn(|b| (r * 29 + b) as u8);
        s.upper[r] = [0xa0 + r as u8; 16];
    }
    s.simd.mxcsr = 0xe5af;
    s
}
#[test]
fn literal_word_multiply_wrap_and_saturating_byte_pairs() {
    let mut s = state();
    s.simd.xmm[1] = [
        0, 128, 0, 128, 0, 128, 0, 128, 0, 128, 0, 128, 0, 128, 0, 128,
    ];
    s.simd.xmm[2] = s.simd.xmm[1];
    s.execute_multiply(
        i(
            op(P::MultiplyAddWords, 2, 0),
            0,
            1,
            MoveSource::Register(r(2)),
        ),
        &[],
    )
    .unwrap();
    assert_eq!(
        s.simd.xmm[0],
        [0, 0, 0, 128, 0, 0, 0, 128, 0, 0, 0, 128, 0, 0, 0, 128]
    );
    s.execute_multiply(
        i(
            op(P::MultiplyHighRoundSigned, 2, 0),
            0,
            1,
            MoveSource::Register(r(2)),
        ),
        &[],
    )
    .unwrap();
    assert_eq!(
        s.simd.xmm[0],
        [
            0, 128, 0, 128, 0, 128, 0, 128, 0, 128, 0, 128, 0, 128, 0, 128
        ]
    );
    s.simd.xmm[1] = [255; 16];
    s.simd.xmm[2] = [
        127, 127, 128, 128, 127, 127, 128, 128, 127, 127, 128, 128, 127, 127, 128, 128,
    ];
    s.execute_multiply(
        i(
            op(P::MultiplyAddUnsignedSignedBytes, 1, 0),
            0,
            1,
            MoveSource::Register(r(2)),
        ),
        &[],
    )
    .unwrap();
    assert_eq!(
        s.simd.xmm[0],
        [
            255, 127, 0, 128, 255, 127, 0, 128, 255, 127, 0, 128, 255, 127, 0, 128
        ]
    );
}
#[test]
fn literal_horizontal_sum_sad_and_minimum_first_tie() {
    let mut s = state();
    s.simd.xmm[1] = [
        255, 127, 1, 0, 0, 128, 255, 255, 0, 0, 1, 0, 255, 255, 255, 255,
    ];
    s.simd.xmm[2] = s.simd.xmm[1];
    s.execute_multiply(
        i(
            op(P::HorizontalAddSignedSaturate, 2, 0),
            0,
            1,
            MoveSource::Register(r(2)),
        ),
        &[],
    )
    .unwrap();
    assert_eq!(
        s.simd.xmm[0],
        [
            255, 127, 0, 128, 1, 0, 254, 255, 255, 127, 0, 128, 1, 0, 254, 255
        ]
    );
    s.simd.xmm[1] = [0; 16];
    s.simd.xmm[2] = [255; 16];
    s.execute_multiply(
        i(
            op(P::SumAbsoluteByteDifferences, 1, 0),
            0,
            1,
            MoveSource::Register(r(2)),
        ),
        &[],
    )
    .unwrap();
    assert_eq!(
        s.simd.xmm[0],
        [248, 7, 0, 0, 0, 0, 0, 0, 248, 7, 0, 0, 0, 0, 0, 0]
    );
    s.simd.xmm[2] = [10, 0, 1, 0, 9, 0, 1, 0, 0, 128, 1, 0, 255, 255, 2, 0];
    s.execute_multiply(
        i(
            op(P::MinimumPositionUnsignedWord, 2, 0),
            0,
            0,
            MoveSource::Register(r(2)),
        ),
        &[],
    )
    .unwrap();
    assert_eq!(
        s.simd.xmm[0],
        [1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    s.simd.xmm[1] = std::array::from_fn(|n| n as u8);
    s.simd.xmm[2] = s.simd.xmm[1];
    s.execute_multiply(
        i(
            op(P::MultipleSumAbsoluteByteDifferences, 1, 7),
            0,
            1,
            MoveSource::Register(r(2)),
        ),
        &[],
    )
    .unwrap();
    assert_eq!(
        s.simd.xmm[0],
        [32, 0, 28, 0, 24, 0, 20, 0, 16, 0, 12, 0, 8, 0, 4, 0]
    );
}
#[test]
fn aliases_exact_memory_and_host_refusal_are_neutral() {
    for (d, a, b) in [(0, 1, 2), (0, 0, 2), (0, 1, 0), (0, 1, 1), (0, 0, 0)] {
        let mut s = state();
        let before = s;
        let first = s.simd.xmm[a as usize];
        let second = s.simd.xmm[b as usize];
        s.execute_multiply(
            i(op(P::MultiplyLow, 4, 0), d, a, MoveSource::Register(r(b))),
            &[],
        )
        .unwrap();
        let mut expected = before;
        for n in 0..4 {
            let x = u32::from_le_bytes(first[n * 4..n * 4 + 4].try_into().unwrap());
            let y = u32::from_le_bytes(second[n * 4..n * 4 + 4].try_into().unwrap());
            expected.simd.xmm[d as usize][n * 4..n * 4 + 4]
                .copy_from_slice(&x.wrapping_mul(y).to_le_bytes());
        }
        expected.upper[d as usize] = [0; 16];
        assert_eq!(s, expected);
    }
    let mut s = state();
    let before = s;
    let instruction = i(op(P::MultiplySignedDword, 4, 0), 0, 1, MoveSource::Memory);
    assert_eq!(
        instruction.memory(),
        Some(MemoryRequirement {
            access: Access::Read,
            bytes: 16,
            alignment: 1
        })
    );
    assert_eq!(
        s.execute_multiply(instruction, &[0; 8]),
        Err(MultiplyError::OperandLength)
    );
    assert_eq!(s, before);
    s.simd.mxcsr = 0x10000;
    let before = s;
    assert_eq!(
        s.execute_multiply(instruction, &[0; 16]),
        Err(MultiplyError::InvalidState)
    );
    assert_eq!(s, before);
}
#[test]
fn closed_eighteen_multiply_forms_reject_incoherent_shapes() {
    let mut forms = 0;
    for kind in [
        P::MultiplyLow,
        P::MultiplyHighSigned,
        P::MultiplyHighUnsigned,
        P::MultiplyUnsignedDword,
        P::MultiplySignedDword,
        P::MultiplyAddWords,
        P::SumAbsoluteByteDifferences,
        P::HorizontalAdd,
        P::HorizontalSubtract,
        P::HorizontalAddSignedSaturate,
        P::HorizontalSubtractSignedSaturate,
        P::MultiplyAddUnsignedSignedBytes,
        P::MultiplyHighRoundSigned,
        P::MultipleSumAbsoluteByteDifferences,
        P::MinimumPositionUnsignedWord,
    ] {
        for width in [1, 2, 4, 8] {
            forms += usize::from(MultiplyOperation::new(kind, width, 0).is_some());
        }
    }
    assert_eq!(forms, 18);
    assert!(MultiplyOperation::new(P::MultiplyLow, 8, 0).is_none());
    assert!(MultiplyOperation::new(P::MultiplyHighRoundSigned, 4, 0).is_none());
    assert!(MultiplyOperation::new(P::MultiplyAddWords, 2, 1).is_none());
    assert!(
        MultiplyInstruction::new(
            op(P::MinimumPositionUnsignedWord, 2, 0),
            r(0),
            Some(r(1)),
            MoveSource::Register(r(2))
        )
        .is_none()
    );
}
