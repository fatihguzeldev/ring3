use super::*;
fn r(i: u8) -> VectorRegister {
    VectorRegister::new(i).unwrap()
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
fn general() -> State32 {
    State32 {
        registers: [0xdeadbeef; 8],
        eip: 0x12345678,
        eflags: 0x244ed7,
    }
}
#[test]
fn insert_masks_index_and_uses_low_scalar_bytes_before_upper_zero() {
    for (width, immediate, at, bytes) in [
        (ScalarWidth::Byte, 255, 15, vec![0xef]),
        (ScalarWidth::Word, 255, 14, vec![0xef, 0xbe]),
        (ScalarWidth::Dword, 255, 12, vec![0xef, 0xbe, 0xad, 0xde]),
    ] {
        for (d, a) in [(0, 1), (0, 0), (7, 7)] {
            let mut s = state();
            let mut g = general();
            let before = s;
            let gbefore = g;
            let mut expected = before;
            expected.simd.xmm[d as usize] = before.simd.xmm[a as usize];
            expected.simd.xmm[d as usize][at..at + bytes.len()].copy_from_slice(&bytes);
            expected.upper[d as usize] = [0; 16];
            s.execute_scalar(
                ScalarInstruction::Insert {
                    width,
                    destination: r(d),
                    merge: r(a),
                    source: WordSource::Register(Register32::Eax),
                    immediate,
                },
                &mut g,
                &mut [],
            )
            .unwrap();
            assert_eq!(s, expected);
            assert_eq!(g, gbefore);
        }
    }
}
#[test]
fn extract_zero_extends_full_gpr_and_mask_preserves_every_vector() {
    let mut s = state();
    let mut g = general();
    s.simd.xmm[1] = [
        0x80, 0x7f, 0x80, 0x7f, 0x80, 0x7f, 0x80, 0x7f, 0x80, 0x7f, 0x80, 0x7f, 0x80, 0x7f, 0x80,
        0xff,
    ];
    let before = s;
    for (width, value) in [
        (ScalarWidth::Byte, 0xff),
        (ScalarWidth::Word, 0xff80),
        (ScalarWidth::Dword, 0xff807f80),
    ] {
        let mut expected = g;
        expected.registers[0] = value;
        s.execute_scalar(
            ScalarInstruction::Extract {
                width,
                source: r(1),
                destination: WordTarget::Register(Register32::Eax),
                immediate: 255,
            },
            &mut g,
            &mut [],
        )
        .unwrap();
        assert_eq!(g, expected);
        assert_eq!(s, before);
    }
    let mut expected = g;
    expected.registers[7] = 0xd555;
    s.execute_scalar(
        ScalarInstruction::Mask {
            width: VectorWidth::Xmm,
            source: r(1),
            destination: Register32::Edi,
        },
        &mut g,
        &mut [],
    )
    .unwrap();
    assert_eq!(g, expected);
    assert_eq!(s, before);
}
#[test]
fn narrow_memory_transport_and_invalid_requests_preserve_state_and_tail() {
    for width in [ScalarWidth::Byte, ScalarWidth::Word, ScalarWidth::Dword] {
        let n = usize::from(width.bytes());
        let mut s = state();
        let mut g = general();
        let input = [0x80, 0xff, 0x12, 0x34];
        let mut operand = input[..n].to_vec();
        let ins = ScalarInstruction::Insert {
            width,
            destination: r(0),
            merge: r(1),
            source: WordSource::Memory,
            immediate: 0,
        };
        assert_eq!(
            ins.memory(),
            Some(MemoryRequirement {
                access: Access::Read,
                bytes: n as u8,
                alignment: 1
            })
        );
        s.execute_scalar(ins, &mut g, &mut operand).unwrap();
        assert_eq!(&s.simd.xmm[0][..n], &input[..n]);
        assert_eq!(operand, input[..n]);
        let before = s;
        let gbefore = g;
        let ext = ScalarInstruction::Extract {
            width,
            source: r(0),
            destination: WordTarget::Memory,
            immediate: 0,
        };
        assert_eq!(
            ext.memory(),
            Some(MemoryRequirement {
                access: Access::Write,
                bytes: n as u8,
                alignment: 1
            })
        );
        operand.fill(0);
        s.execute_scalar(ext, &mut g, &mut operand).unwrap();
        assert_eq!(operand, input[..n]);
        assert_eq!(s, before);
        assert_eq!(g, gbefore);
        let mut wrong = vec![0x55; n + 1];
        let wrongbefore = wrong.clone();
        assert_eq!(
            s.execute_scalar(ext, &mut g, &mut wrong),
            Err(ScalarError::OperandLength)
        );
        assert_eq!(s, before);
        assert_eq!(g, gbefore);
        assert_eq!(wrong, wrongbefore);
        s.simd.mxcsr = 0x10000;
        let before = s;
        let obefore = operand.clone();
        assert_eq!(
            s.execute_scalar(ext, &mut g, &mut operand),
            Err(ScalarError::InvalidState)
        );
        assert_eq!(s, before);
        assert_eq!(g, gbefore);
        assert_eq!(operand, obefore);
    }
}
