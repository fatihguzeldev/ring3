use super::*;
use crate::cpu::x86::simd::{RearrangeOperation, ShuffleKind as S};

fn r(index: u8) -> VectorRegister {
    VectorRegister::new(index).unwrap()
}
fn op(kind: S, width: u8, immediate: u8) -> RearrangeOperation {
    RearrangeOperation::new(kind, width, immediate).unwrap()
}
fn instruction(
    operation: RearrangeOperation,
    d: u8,
    a: u8,
    b: MoveSource,
    mask: Option<u8>,
) -> RearrangeInstruction {
    RearrangeInstruction::new(
        operation,
        r(d),
        (!operation.unary()).then_some(r(a)),
        b,
        mask.map(r),
    )
    .unwrap()
}
fn state() -> VectorState {
    let mut state = VectorState::default();
    for index in 0..8 {
        state.simd.xmm[index] = std::array::from_fn(|b| (index * 29 + b) as u8);
        state.upper[index] = [0xa0 + index as u8; 16];
    }
    state.simd.mxcsr = 0xe5af;
    state
}

#[test]
fn literal_unpack_pack_and_widen_boundaries() {
    let mut s = state();
    s.execute_rearrange(
        instruction(
            op(S::UnpackHigh, 4, 0),
            0,
            1,
            MoveSource::Register(r(2)),
            None,
        ),
        &[],
    )
    .unwrap();
    assert_eq!(
        s.simd.xmm[0],
        [
            37, 38, 39, 40, 66, 67, 68, 69, 41, 42, 43, 44, 70, 71, 72, 73
        ]
    );
    s.simd.xmm[1] = [
        0, 128, 255, 127, 0, 0, 255, 255, 128, 255, 127, 0, 0, 1, 255, 0,
    ];
    s.simd.xmm[2] = [
        0, 128, 255, 127, 0, 0, 255, 255, 128, 255, 127, 0, 0, 1, 255, 0,
    ];
    s.execute_rearrange(
        instruction(
            op(S::PackUnsigned, 1, 0),
            0,
            1,
            MoveSource::Register(r(2)),
            None,
        ),
        &[],
    )
    .unwrap();
    assert_eq!(
        s.simd.xmm[0],
        [
            0, 255, 0, 0, 0, 127, 255, 255, 0, 255, 0, 0, 0, 127, 255, 255
        ]
    );
    s.execute_rearrange(
        instruction(op(S::SignExtendBytes, 8, 0), 0, 0, MoveSource::Memory, None),
        &[0x80, 0x7f],
    )
    .unwrap();
    assert_eq!(
        s.simd.xmm[0],
        [
            0x80, 255, 255, 255, 255, 255, 255, 255, 0x7f, 0, 0, 0, 0, 0, 0, 0
        ]
    );
    assert_eq!(s.upper[0], [0; 16]);
}

#[test]
fn literal_shuffle_align_and_explicit_mask_source() {
    let mut s = state();
    s.execute_rearrange(
        instruction(
            op(S::Dwords, 4, 0x1b),
            0,
            0,
            MoveSource::Register(r(1)),
            None,
        ),
        &[],
    )
    .unwrap();
    assert_eq!(
        s.simd.xmm[0],
        [
            41, 42, 43, 44, 37, 38, 39, 40, 33, 34, 35, 36, 29, 30, 31, 32
        ]
    );
    s.execute_rearrange(
        instruction(
            op(S::AlignRight, 1, 31),
            0,
            1,
            MoveSource::Register(r(2)),
            None,
        ),
        &[],
    )
    .unwrap();
    assert_eq!(
        s.simd.xmm[0],
        [44, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    s.simd.xmm[3] = [0x80, 0x7f, 0xff, 0, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    s.execute_rearrange(
        instruction(
            op(S::BlendVariable, 1, 0),
            0,
            1,
            MoveSource::Register(r(2)),
            Some(3),
        ),
        &[],
    )
    .unwrap();
    assert_eq!(
        s.simd.xmm[0],
        [
            58, 30, 60, 32, 62, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44
        ]
    );
}

#[test]
fn aliases_capture_all_sources_before_publication() {
    for kind in [
        S::UnpackLow,
        S::PackSigned,
        S::Bytes,
        S::AlignRight,
        S::BlendImmediate,
        S::BlendVariable,
    ] {
        let width = if kind == S::BlendImmediate { 2 } else { 1 };
        let operation = op(
            kind,
            width,
            if matches!(kind, S::AlignRight | S::BlendImmediate) {
                7
            } else {
                0
            },
        );
        for (d, a, b, m) in [
            (0, 1, 2, 3),
            (0, 0, 2, 3),
            (0, 1, 0, 3),
            (0, 1, 1, 3),
            (0, 0, 0, 0),
            (0, 1, 2, 0),
        ] {
            let mut s = state();
            let before = s;
            let mut detached = before;
            detached.simd.xmm[4] = before.simd.xmm[a as usize];
            detached.simd.xmm[5] = before.simd.xmm[b as usize];
            detached.simd.xmm[6] = before.simd.xmm[m as usize];
            detached
                .execute_rearrange(
                    instruction(
                        operation,
                        7,
                        4,
                        MoveSource::Register(r(5)),
                        (kind == S::BlendVariable).then_some(6),
                    ),
                    &[],
                )
                .unwrap();
            s.execute_rearrange(
                instruction(
                    operation,
                    d,
                    a,
                    MoveSource::Register(r(b)),
                    (kind == S::BlendVariable).then_some(m),
                ),
                &[],
            )
            .unwrap();
            let mut expected = before;
            expected.simd.xmm[d as usize] = detached.simd.xmm[7];
            expected.upper[d as usize] = [0; 16];
            assert_eq!(s, expected, "{kind:?}:{d}/{a}/{b}/{m}");
        }
    }
}

#[test]
fn exact_widening_spans_and_invalid_host_requests_are_neutral() {
    for (kind, to, bytes) in [
        (S::SignExtendBytes, 2, 8),
        (S::SignExtendBytes, 4, 4),
        (S::SignExtendBytes, 8, 2),
        (S::ZeroExtendWords, 4, 8),
        (S::ZeroExtendWords, 8, 4),
        (S::ZeroExtendDwords, 8, 8),
    ] {
        let i = instruction(op(kind, to, 0), 0, 0, MoveSource::Memory, None);
        assert_eq!(
            i.memory(),
            Some(MemoryRequirement {
                access: Access::Read,
                bytes,
                alignment: 1
            })
        );
        let mut s = state();
        let before = s;
        assert_eq!(
            s.execute_rearrange(i, &vec![0; usize::from(bytes) - 1]),
            Err(RearrangeError::OperandLength)
        );
        assert_eq!(s, before);
        s.execute_rearrange(i, &vec![0x80; usize::from(bytes)])
            .unwrap();
        assert_eq!(s.upper[0], [0; 16]);
    }
    let mut s = state();
    s.simd.mxcsr = 0x10000;
    let before = s;
    assert_eq!(
        s.execute_rearrange(
            instruction(
                op(S::UnpackLow, 1, 0),
                0,
                1,
                MoveSource::Register(r(2)),
                None
            ),
            &[]
        ),
        Err(RearrangeError::InvalidState)
    );
    assert_eq!(s, before);
}

#[test]
fn typed_surface_rejects_non_avx_or_incoherent_forms() {
    for (kind, width, imm) in [
        (S::Words, 2, 0),
        (S::UnpackLow, 3, 0),
        (S::PackSigned, 4, 0),
        (S::SignExtendDwords, 4, 0),
        (S::BlendVariable, 4, 0),
        (S::Bytes, 1, 1),
    ] {
        assert!(RearrangeOperation::new(kind, width, imm).is_none());
    }
    assert!(
        RearrangeInstruction::new(
            op(S::Dwords, 4, 0),
            r(0),
            Some(r(1)),
            MoveSource::Register(r(2)),
            None
        )
        .is_none()
    );
    assert!(
        RearrangeInstruction::new(
            op(S::BlendVariable, 1, 0),
            r(0),
            Some(r(1)),
            MoveSource::Register(r(2)),
            None
        )
        .is_none()
    );
    assert!(
        RearrangeInstruction::new(
            op(S::UnpackLow, 1, 0),
            r(0),
            Some(r(1)),
            MoveSource::Register(r(2)),
            Some(r(3))
        )
        .is_none()
    );
}
