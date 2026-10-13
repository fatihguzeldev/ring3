use super::*;
use crate::cpu::x86::simd::SimdState;
fn r(i: u8) -> VectorRegister {
    VectorRegister::new(i).unwrap()
}
fn hex(s: &str) -> [u8; 16] {
    assert_eq!(s.len(), 32);
    std::array::from_fn(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap())
}
fn state() -> VectorState {
    VectorState {
        simd: SimdState {
            mxcsr: 0xe5af,
            xmm: std::array::from_fn(|r| std::array::from_fn(|b| (r * 17 + b * 29 + 7) as u8)),
        },
        upper: std::array::from_fn(|r| std::array::from_fn(|b| (r * 31 + b * 19 + 3) as u8)),
    }
}
fn kinds() -> [CryptoKind; 7] {
    [
        CryptoKind::EncryptRound,
        CryptoKind::EncryptLast,
        CryptoKind::DecryptRound,
        CryptoKind::DecryptLast,
        CryptoKind::InverseMixColumns,
        CryptoKind::KeyAssist,
        CryptoKind::CarrylessMultiply,
    ]
}
fn instruction(kind: CryptoKind, d: u8, a: u8, second: MoveSource, imm: u8) -> CryptoInstruction {
    let first = if matches!(kind, CryptoKind::InverseMixColumns | CryptoKind::KeyAssist) {
        None
    } else {
        Some(r(a))
    };
    let immediate =
        matches!(kind, CryptoKind::KeyAssist | CryptoKind::CarrylessMultiply).then_some(imm);
    CryptoInstruction::new(kind, r(d), first, second, immediate).unwrap()
}
fn expected(mut before: VectorState, d: u8, value: [u8; 16]) -> VectorState {
    before.simd.xmm[d as usize] = value;
    before.upper[d as usize] = [0; 16];
    before
}
#[test]
fn constructors_close_binary_unary_and_immediate_shapes() {
    for kind in kinds() {
        for first in [None, Some(r(1))] {
            for immediate in [None, Some(0)] {
                let binary = !matches!(kind, CryptoKind::InverseMixColumns | CryptoKind::KeyAssist);
                let controlled =
                    matches!(kind, CryptoKind::KeyAssist | CryptoKind::CarrylessMultiply);
                assert_eq!(
                    CryptoInstruction::new(
                        kind,
                        r(0),
                        first,
                        MoveSource::Register(r(2)),
                        immediate
                    )
                    .is_some(),
                    first.is_some() == binary && immediate.is_some() == controlled
                );
            }
        }
        let memory = instruction(kind, 0, 1, MoveSource::Memory, 0)
            .memory()
            .unwrap();
        assert_eq!(
            (memory.access, memory.bytes, memory.alignment),
            (Access::Read, 16, 1)
        );
        assert_eq!(
            instruction(kind, 0, 1, MoveSource::Register(r(2)), 0).memory(),
            None
        );
    }
}
#[test]
fn nist_round_and_inverse_mix_values_preserve_every_unrelated_vector_byte() {
    for (kind, a, b, result) in [
        (
            CryptoKind::EncryptRound,
            "00102030405060708090a0b0c0d0e0f0",
            "d6aa74fdd2af72fadaa678f1d6ab76fe",
            "89d810e8855ace682d1843d8cb128fe4",
        ),
        (
            CryptoKind::EncryptLast,
            "bd6e7c3df2b5779e0b61216e8b10b689",
            "13111d7fe3944a17f307a78b4d2b30c5",
            "69c4e0d86a7b0430d8cdb78070b4c55a",
        ),
        (
            CryptoKind::DecryptRound,
            "7ad5fda789ef4e272bca100b3d9ff59f",
            "13aa29be9c8faff6f770f58000f7bf03",
            "54d990a16ba09ab596bbf40ea111702f",
        ),
        (
            CryptoKind::DecryptLast,
            "6353e08c0960e104cd70b751bacad0e7",
            "000102030405060708090a0b0c0d0e0f",
            "00112233445566778899aabbccddeeff",
        ),
        (
            CryptoKind::InverseMixColumns,
            "00000000000000000000000000000000",
            "549932d1f08557681093ed9cbe2c974e",
            "13aa29be9c8faff6f770f58000f7bf03",
        ),
    ] {
        for d in [0, 1, 2, 7] {
            for memory in [false, true] {
                let mut s = state();
                s.simd.xmm[1] = hex(a);
                s.simd.xmm[2] = hex(b);
                let before = s;
                let operand = hex(b);
                s.execute_crypto(
                    instruction(
                        kind,
                        d,
                        1,
                        if memory {
                            MoveSource::Memory
                        } else {
                            MoveSource::Register(r(2))
                        },
                        0,
                    ),
                    if memory { &operand[..] } else { &[] },
                )
                .unwrap();
                assert_eq!(
                    s,
                    expected(before, d, hex(result)),
                    "{kind:?}/d{d}/memory{memory}"
                );
            }
        }
    }
    for kind in [
        CryptoKind::EncryptRound,
        CryptoKind::EncryptLast,
        CryptoKind::DecryptRound,
        CryptoKind::DecryptLast,
    ] {
        for d in 0..8 {
            for a in 0..8 {
                for b in 0..8 {
                    let mut s = state();
                    s.simd.xmm[a as usize] = [0; 16];
                    s.simd.xmm[b as usize] = [0; 16];
                    let before = s;
                    s.execute_crypto(instruction(kind, d, a, MoveSource::Register(r(b)), 0), &[])
                        .unwrap();
                    let byte = if matches!(kind, CryptoKind::EncryptRound | CryptoKind::EncryptLast)
                    {
                        0x63
                    } else {
                        0x52
                    };
                    assert_eq!(s, expected(before, d, [byte; 16]));
                }
            }
        }
    }
}
#[test]
fn every_key_constant_and_carryless_selector_uses_literal_polynomial_results() {
    for immediate in 0..=255 {
        for d in [0, 1, 7] {
            let mut s = state();
            s.simd.xmm[1] = hex("000102030405060708090a0b0c0d0e0f");
            let before = s;
            let mut value = hex("f26b6fc56b6fc5f2fed7ab76d7ab76fe");
            value[4] ^= immediate;
            value[12] ^= immediate;
            s.execute_crypto(
                instruction(
                    CryptoKind::KeyAssist,
                    d,
                    0,
                    MoveSource::Register(r(1)),
                    immediate,
                ),
                &[],
            )
            .unwrap();
            assert_eq!(s, expected(before, d, value));
        }
        for same in [false, true] {
            for d in [0, 1, 2, 7] {
                let mut s = state();
                s.simd.xmm[1] = [1u64 << 63, 3]
                    .map(u64::to_le_bytes)
                    .concat()
                    .try_into()
                    .unwrap();
                s.simd.xmm[2] = [2u64, 0x8000000000000001]
                    .map(u64::to_le_bytes)
                    .concat()
                    .try_into()
                    .unwrap();
                let before = s;
                let value = if same {
                    match immediate & 0x11 {
                        0 => [0u64, 0x4000000000000000],
                        0x11 => [5, 0],
                        _ => [0x8000000000000000, 1],
                    }
                } else {
                    match immediate & 0x11 {
                        0 => [0, 1],
                        1 => [6, 0],
                        0x10 => [0x8000000000000000, 0x4000000000000000],
                        0x11 => [0x8000000000000003, 1],
                        _ => unreachable!(),
                    }
                };
                s.execute_crypto(
                    instruction(
                        CryptoKind::CarrylessMultiply,
                        d,
                        1,
                        MoveSource::Register(r(if same { 1 } else { 2 })),
                        immediate,
                    ),
                    &[],
                )
                .unwrap();
                assert_eq!(
                    s,
                    expected(
                        before,
                        d,
                        value.map(u64::to_le_bytes).concat().try_into().unwrap()
                    )
                );
            }
        }
    }
}
#[test]
fn exact_operand_and_invalid_host_state_fail_before_destination_publication() {
    for kind in kinds() {
        let i = instruction(kind, 0, 1, MoveSource::Memory, 0);
        for length in [0, 15, 17, 32] {
            let mut s = state();
            let before = s;
            assert_eq!(
                s.execute_crypto(i, &[0xa5; 32][..length]),
                Err(ArithmeticError::OperandLength)
            );
            assert_eq!(s, before);
        }
        let mut s = state();
        s.simd.mxcsr = 0x10000;
        let before = s;
        assert_eq!(
            s.execute_crypto(i, &[0; 16]),
            Err(ArithmeticError::InvalidState)
        );
        assert_eq!(s, before);
        let i = instruction(kind, 0, 1, MoveSource::Register(r(2)), 0);
        let mut s = state();
        let before = s;
        assert_eq!(
            s.execute_crypto(i, &[0; 16]),
            Err(ArithmeticError::OperandLength)
        );
        assert_eq!(s, before);
    }
}
