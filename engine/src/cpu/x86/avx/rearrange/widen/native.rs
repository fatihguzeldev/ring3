use super::*;
use crate::cpu::x86::simd::ShuffleKind;

fn forms() -> Vec<RearrangeOperation> {
    [
        ShuffleKind::SignExtendBytes,
        ShuffleKind::SignExtendWords,
        ShuffleKind::SignExtendDwords,
        ShuffleKind::ZeroExtendBytes,
        ShuffleKind::ZeroExtendWords,
        ShuffleKind::ZeroExtendDwords,
    ]
    .into_iter()
    .flat_map(|kind| {
        [2, 4, 8]
            .into_iter()
            .filter_map(move |to| RearrangeOperation::new(kind, to, 0))
    })
    .collect()
}

#[test]
fn widths_are_closed_to_widening_and_report_exact_low_xmm_source_bytes() {
    let forms = forms();
    assert_eq!(forms.len(), 12);
    for operation in forms {
        let narrow =
            RearrangeInstruction::new(operation, VectorRegister(0), None, MoveSource::Memory, None)
                .unwrap();
        let wide = narrow.with_width(VectorWidth::Ymm).unwrap();
        let (from, _) = operation.kind().widening().unwrap();
        assert_eq!(narrow.width(), VectorWidth::Xmm);
        assert_eq!(wide.width(), VectorWidth::Ymm);
        assert_eq!(narrow.source_bytes(), 16 / operation.lane_bytes() * from);
        assert_eq!(wide.source_bytes(), 32 / operation.lane_bytes() * from);
        assert_eq!(wide.with_width(VectorWidth::Xmm), Some(narrow));
        assert_eq!(
            wide.memory(),
            Some(MemoryRequirement {
                access: Access::Read,
                bytes: wide.source_bytes(),
                alignment: 1
            })
        );
    }
    for operation in [
        RearrangeOperation::new(ShuffleKind::PackUnsigned, 2, 0).unwrap(),
        RearrangeOperation::new(ShuffleKind::UnpackHigh, 4, 0).unwrap(),
        RearrangeOperation::new(ShuffleKind::Dwords, 4, 0xff).unwrap(),
    ] {
        let narrow = RearrangeInstruction::new(
            operation,
            VectorRegister(0),
            (!operation.unary()).then_some(VectorRegister(1)),
            MoveSource::Register(VectorRegister(2)),
            None,
        )
        .unwrap();
        assert!(narrow.with_width(VectorWidth::Ymm).is_none());
    }
}

fn expected(operation: RearrangeOperation, input: [u8; 16]) -> [u8; 32] {
    let (from, signed) = operation.kind().widening().unwrap();
    let from = usize::from(from);
    let to = usize::from(operation.lane_bytes());
    let mut value = [0; 32];
    for lane in 0..32 / to {
        let source = &input[lane * from..(lane + 1) * from];
        let output = &mut value[lane * to..(lane + 1) * to];
        output.fill(if signed && source[from - 1] & 0x80 != 0 {
            0xff
        } else {
            0
        });
        output[..from].copy_from_slice(source);
    }
    value
}

#[test]
fn both_output_halves_use_contiguous_captured_low_xmm_bytes_under_aliases() {
    let input = [
        0x80, 0x7f, 0xff, 1, 0, 0xfe, 0x81, 0x7e, 2, 0xfd, 0x82, 0x7d, 0x12, 0xed, 0x23, 0xdc,
    ];
    for operation in forms() {
        for source in 0..8 {
            for destination in 0..8 {
                for canary in [0x35, 0xca] {
                    let mut state = VectorState::default();
                    state.simd.mxcsr = 0xffff;
                    state.simd.xmm = std::array::from_fn(|r| [r as u8; 16]);
                    state.upper = std::array::from_fn(|r| [r as u8 ^ canary; 16]);
                    state.simd.xmm[source] = input;
                    let before = state;
                    let instruction = RearrangeInstruction::new(
                        operation,
                        VectorRegister(destination as u8),
                        None,
                        MoveSource::Register(VectorRegister(source as u8)),
                        None,
                    )
                    .unwrap()
                    .with_width(VectorWidth::Ymm)
                    .unwrap();
                    state.execute_rearrange(instruction, &[]).unwrap();
                    let value = expected(operation, input);
                    assert_eq!(state.simd.xmm[destination], value[..16]);
                    assert_eq!(state.upper[destination], value[16..]);
                    assert_eq!(state.simd.mxcsr, before.simd.mxcsr);
                    for register in (0..8).filter(|r| *r != destination) {
                        assert_eq!(state.simd.xmm[register], before.simd.xmm[register]);
                        assert_eq!(state.upper[register], before.upper[register]);
                    }
                }
            }
        }
    }
}

#[test]
fn memory_width_and_invalid_mxcsr_refusals_publish_no_partial_vector() {
    for operation in forms() {
        let instruction =
            RearrangeInstruction::new(operation, VectorRegister(0), None, MoveSource::Memory, None)
                .unwrap()
                .with_width(VectorWidth::Ymm)
                .unwrap();
        let length = usize::from(instruction.source_bytes());
        for supplied in [length - 1, length + 1] {
            let mut state = VectorState::default();
            state.upper[0] = [0x91; 16];
            let before = state;
            assert_eq!(
                state.execute_rearrange(instruction, &vec![0x80; supplied]),
                Err(RearrangeError::OperandLength)
            );
            assert_eq!(state.simd, before.simd);
            assert_eq!(state.upper, before.upper);
        }
        let mut state = VectorState::default();
        state.simd.mxcsr = 0x1_ffff;
        let before = state;
        assert_eq!(
            state.execute_rearrange(instruction, &vec![0x80; length]),
            Err(RearrangeError::InvalidState)
        );
        assert_eq!(state.simd, before.simd);
        assert_eq!(state.upper, before.upper);
        state.simd.mxcsr = 0xffff;
        let input = [0x80; 16];
        state
            .execute_rearrange(instruction, &input[..length])
            .unwrap();
        assert_eq!(state.simd.xmm[0], expected(operation, input)[..16]);
        assert_eq!(state.upper[0], expected(operation, input)[16..]);
    }
}
