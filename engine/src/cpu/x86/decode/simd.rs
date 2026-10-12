use iced_x86::{EncodingKind, Instruction, Mnemonic, OpKind, Register};

use super::{
    DecodeError,
    operands::{effective_address, register, unsupported},
};
use crate::cpu::x86::simd::{
    ConvertKind as C, FloatKind as F, PackedKind as P, ShuffleKind as S, SimdKind as K,
    SimdOperand as O, SimdOperation, SimdRegister as R,
};

fn operand(instruction: &Instruction, index: u32) -> Result<O, DecodeError> {
    match instruction.op_kind(index) {
        OpKind::Register => {
            let raw = instruction.op_register(index);
            let register = if (Register::XMM0..=Register::XMM7).contains(&raw) {
                R::Xmm((raw as u32 - Register::XMM0 as u32) as u8)
            } else if (Register::MM0..=Register::MM7).contains(&raw) {
                R::Mmx((raw as u32 - Register::MM0 as u32) as u8)
            } else {
                R::Gpr(register(raw)?)
            };
            Ok(O::Register(register))
        }
        OpKind::Memory => Ok(O::Memory(effective_address(instruction)?)),
        OpKind::Immediate8 => Ok(O::None),
        _ => Err(unsupported()),
    }
}

pub(super) fn lower(instruction: &Instruction) -> Option<Result<SimdOperation, DecodeError>> {
    use Mnemonic::*;
    if instruction.encoding() != EncodingKind::Legacy {
        return None;
    }
    let (kind, lane_bytes, scalar, aligned) = match instruction.mnemonic() {
        Maskmovq | Maskmovdqu => (K::MaskedStore, 1, false, false),
        Sfence | Lfence | Mfence => (K::Fence, 0, false, false),
        Prefetchnta | Prefetcht0 | Prefetcht1 | Prefetcht2 => (K::Prefetch, 0, false, false),
        Movntps | Movntpd | Movntdq => (K::Move, 16, false, true),
        Movntq => (K::Move, 8, false, false),
        Movnti => (K::Move, 4, false, false),
        Fxsave => (K::SaveExtended, 0, false, true),
        Fxrstor => (K::RestoreExtended, 0, false, true),
        Emms => (K::EmptyMmx, 0, false, false),
        Ldmxcsr => (K::LoadMxcsr, 4, false, false),
        Stmxcsr => (K::StoreMxcsr, 4, false, false),
        Movaps | Movapd | Movdqa | Movntdqa => (K::Move, 16, false, true),
        Movups | Movupd | Movdqu | Lddqu => (K::Move, 16, false, false),
        Movq | Movdq2q | Movq2dq => (K::Move, 8, false, false),
        Movd => (K::Move, 4, false, false),
        Movss => (K::MoveScalar, 4, true, false),
        Movsd if instruction.op_count() == 2 => (K::MoveScalar, 8, true, false),
        Movlps | Movlpd => (K::MoveLow, 8, false, false),
        Movhps | Movhpd => (K::MoveHigh, 8, false, false),
        Movddup => (K::Shuffle(S::DuplicateEven), 8, false, false),
        Movsldup => (K::Shuffle(S::DuplicateEven), 4, false, false),
        Movshdup => (K::Shuffle(S::DuplicateOdd), 4, false, false),
        Addsubps => (K::Float(F::AddSubtract), 4, false, false),
        Addsubpd => (K::Float(F::AddSubtract), 8, false, false),
        Haddps => (K::Float(F::HorizontalAdd), 4, false, false),
        Haddpd => (K::Float(F::HorizontalAdd), 8, false, false),
        Hsubps => (K::Float(F::HorizontalSubtract), 4, false, false),
        Hsubpd => (K::Float(F::HorizontalSubtract), 8, false, false),
        Movlhps => (K::Shuffle(S::MoveLowToHigh), 8, false, false),
        Movhlps => (K::Shuffle(S::MoveHighToLow), 8, false, false),
        Pmovsxbw => (K::Shuffle(S::SignExtendBytes), 2, false, false),
        Pmovsxbd => (K::Shuffle(S::SignExtendBytes), 4, false, false),
        Pmovsxbq => (K::Shuffle(S::SignExtendBytes), 8, false, false),
        Pmovsxwd => (K::Shuffle(S::SignExtendWords), 4, false, false),
        Pmovsxwq => (K::Shuffle(S::SignExtendWords), 8, false, false),
        Pmovsxdq => (K::Shuffle(S::SignExtendDwords), 8, false, false),
        Pmovzxbw => (K::Shuffle(S::ZeroExtendBytes), 2, false, false),
        Pmovzxbd => (K::Shuffle(S::ZeroExtendBytes), 4, false, false),
        Pmovzxbq => (K::Shuffle(S::ZeroExtendBytes), 8, false, false),
        Pmovzxwd => (K::Shuffle(S::ZeroExtendWords), 4, false, false),
        Pmovzxwq => (K::Shuffle(S::ZeroExtendWords), 8, false, false),
        Pmovzxdq => (K::Shuffle(S::ZeroExtendDwords), 8, false, false),
        Pblendw => (K::Shuffle(S::BlendImmediate), 2, false, false),
        Blendps => (K::Shuffle(S::BlendImmediate), 4, false, false),
        Blendpd => (K::Shuffle(S::BlendImmediate), 8, false, false),
        Pblendvb => (K::Shuffle(S::BlendVariable), 1, false, false),
        Blendvps => (K::Shuffle(S::BlendVariable), 4, false, false),
        Blendvpd => (K::Shuffle(S::BlendVariable), 8, false, false),
        Pinsrb => (K::Shuffle(S::InsertByte), 1, false, false),
        Pinsrd => (K::Shuffle(S::InsertDword), 4, false, false),
        Insertps => (K::Shuffle(S::InsertSingle), 4, false, false),
        Pextrb => (K::Shuffle(S::ExtractByte), 1, false, false),
        Pextrd | Extractps => (K::Shuffle(S::ExtractDword), 4, false, false),
        Pcmpeqq => (K::Packed(P::Equal), 8, false, false),
        Pcmpgtq => (K::Packed(P::GreaterSigned), 8, false, false),
        Pmuldq => (K::Packed(P::MultiplySignedDword), 4, false, false),
        Pmulld => (K::Packed(P::MultiplyLow), 4, false, false),
        Pminsb => (K::Packed(P::MinimumSigned), 1, false, false),
        Pminsd => (K::Packed(P::MinimumSigned), 4, false, false),
        Pmaxsb => (K::Packed(P::MaximumSigned), 1, false, false),
        Pmaxsd => (K::Packed(P::MaximumSigned), 4, false, false),
        Pminuw => (K::Packed(P::MinimumUnsigned), 2, false, false),
        Pminud => (K::Packed(P::MinimumUnsigned), 4, false, false),
        Pmaxuw => (K::Packed(P::MaximumUnsigned), 2, false, false),
        Pmaxud => (K::Packed(P::MaximumUnsigned), 4, false, false),
        Packusdw => (K::Shuffle(S::PackUnsigned), 2, false, false),
        Pabsb => (K::Packed(P::Absolute), 1, false, false),
        Pabsw => (K::Packed(P::Absolute), 2, false, false),
        Pabsd => (K::Packed(P::Absolute), 4, false, false),
        Psignb => (K::Packed(P::Sign), 1, false, false),
        Psignw => (K::Packed(P::Sign), 2, false, false),
        Psignd => (K::Packed(P::Sign), 4, false, false),
        Phaddw => (K::Packed(P::HorizontalAdd), 2, false, false),
        Phaddd => (K::Packed(P::HorizontalAdd), 4, false, false),
        Phaddsw => (K::Packed(P::HorizontalAddSignedSaturate), 2, false, false),
        Phsubw => (K::Packed(P::HorizontalSubtract), 2, false, false),
        Phsubd => (K::Packed(P::HorizontalSubtract), 4, false, false),
        Phsubsw => (
            K::Packed(P::HorizontalSubtractSignedSaturate),
            2,
            false,
            false,
        ),
        Pmaddubsw => (
            K::Packed(P::MultiplyAddUnsignedSignedBytes),
            1,
            false,
            false,
        ),
        Pmulhrsw => (K::Packed(P::MultiplyHighRoundSigned), 2, false, false),
        Pshufb => (K::Shuffle(S::Bytes), 1, false, false),
        Palignr => (K::Shuffle(S::AlignRight), 1, false, false),
        Paddb => (K::Packed(P::Add), 1, false, false),
        Paddw => (K::Packed(P::Add), 2, false, false),
        Paddd => (K::Packed(P::Add), 4, false, false),
        Paddq => (K::Packed(P::Add), 8, false, false),
        Psubb => (K::Packed(P::Subtract), 1, false, false),
        Psubw => (K::Packed(P::Subtract), 2, false, false),
        Psubd => (K::Packed(P::Subtract), 4, false, false),
        Psubq => (K::Packed(P::Subtract), 8, false, false),
        Paddsb => (K::Packed(P::AddSignedSaturate), 1, false, false),
        Paddsw => (K::Packed(P::AddSignedSaturate), 2, false, false),
        Paddusb => (K::Packed(P::AddUnsignedSaturate), 1, false, false),
        Paddusw => (K::Packed(P::AddUnsignedSaturate), 2, false, false),
        Psubsb => (K::Packed(P::SubtractSignedSaturate), 1, false, false),
        Psubsw => (K::Packed(P::SubtractSignedSaturate), 2, false, false),
        Psubusb => (K::Packed(P::SubtractUnsignedSaturate), 1, false, false),
        Psubusw => (K::Packed(P::SubtractUnsignedSaturate), 2, false, false),
        Pand | Andps | Andpd => (K::Packed(P::And), 8, false, false),
        Pandn | Andnps | Andnpd => (K::Packed(P::AndNot), 8, false, false),
        Por | Orps | Orpd => (K::Packed(P::Or), 8, false, false),
        Pxor | Xorps | Xorpd => (K::Packed(P::Xor), 8, false, false),
        Pcmpeqb => (K::Packed(P::Equal), 1, false, false),
        Pcmpeqw => (K::Packed(P::Equal), 2, false, false),
        Pcmpeqd => (K::Packed(P::Equal), 4, false, false),
        Pcmpgtb => (K::Packed(P::GreaterSigned), 1, false, false),
        Pcmpgtw => (K::Packed(P::GreaterSigned), 2, false, false),
        Pcmpgtd => (K::Packed(P::GreaterSigned), 4, false, false),
        Pmullw => (K::Packed(P::MultiplyLow), 2, false, false),
        Pmulhw => (K::Packed(P::MultiplyHighSigned), 2, false, false),
        Pmulhuw => (K::Packed(P::MultiplyHighUnsigned), 2, false, false),
        Pmuludq => (K::Packed(P::MultiplyUnsignedDword), 4, false, false),
        Pmaddwd => (K::Packed(P::MultiplyAddWords), 2, false, false),
        Pavgb => (K::Packed(P::AverageUnsigned), 1, false, false),
        Pavgw => (K::Packed(P::AverageUnsigned), 2, false, false),
        Pminub => (K::Packed(P::MinimumUnsigned), 1, false, false),
        Pmaxub => (K::Packed(P::MaximumUnsigned), 1, false, false),
        Pminsw => (K::Packed(P::MinimumSigned), 2, false, false),
        Pmaxsw => (K::Packed(P::MaximumSigned), 2, false, false),
        Psadbw => (K::Packed(P::SumAbsoluteByteDifferences), 1, false, false),
        Psllw => (K::Packed(P::ShiftLeft), 2, false, false),
        Pslld => (K::Packed(P::ShiftLeft), 4, false, false),
        Psllq => (K::Packed(P::ShiftLeft), 8, false, false),
        Psrlw => (K::Packed(P::ShiftRight), 2, false, false),
        Psrld => (K::Packed(P::ShiftRight), 4, false, false),
        Psrlq => (K::Packed(P::ShiftRight), 8, false, false),
        Psraw => (K::Packed(P::ShiftRightSigned), 2, false, false),
        Psrad => (K::Packed(P::ShiftRightSigned), 4, false, false),
        Pslldq => (K::Shuffle(S::ShiftBytesLeft), 1, false, false),
        Psrldq => (K::Shuffle(S::ShiftBytesRight), 1, false, false),
        Pshufd => (K::Shuffle(S::Dwords), 4, false, false),
        Pshufw => (K::Shuffle(S::Words), 2, false, false),
        Pshufhw => (K::Shuffle(S::HighWords), 2, false, false),
        Pshuflw => (K::Shuffle(S::LowWords), 2, false, false),
        Shufps => (K::Shuffle(S::Singles), 4, false, false),
        Shufpd => (K::Shuffle(S::Doubles), 8, false, false),
        Punpcklbw => (K::Shuffle(S::UnpackLow), 1, false, false),
        Punpcklwd => (K::Shuffle(S::UnpackLow), 2, false, false),
        Punpckldq | Unpcklps => (K::Shuffle(S::UnpackLow), 4, false, false),
        Punpcklqdq | Unpcklpd => (K::Shuffle(S::UnpackLow), 8, false, false),
        Punpckhbw => (K::Shuffle(S::UnpackHigh), 1, false, false),
        Punpckhwd => (K::Shuffle(S::UnpackHigh), 2, false, false),
        Punpckhdq | Unpckhps => (K::Shuffle(S::UnpackHigh), 4, false, false),
        Punpckhqdq | Unpckhpd => (K::Shuffle(S::UnpackHigh), 8, false, false),
        Packsswb => (K::Shuffle(S::PackSigned), 1, false, false),
        Packssdw => (K::Shuffle(S::PackSigned), 2, false, false),
        Packuswb => (K::Shuffle(S::PackUnsigned), 1, false, false),
        Pinsrw => (K::Shuffle(S::InsertWord), 2, false, false),
        Pextrw => (K::Shuffle(S::ExtractWord), 2, false, false),
        Pmovmskb => (K::Shuffle(S::MaskBytes), 1, false, false),
        Movmskps => (K::Shuffle(S::MaskSingles), 4, false, false),
        Movmskpd => (K::Shuffle(S::MaskDoubles), 8, false, false),
        Addps | Addss => (K::Float(F::Add), 4, instruction.mnemonic() == Addss, false),
        Addpd | Addsd => (K::Float(F::Add), 8, instruction.mnemonic() == Addsd, false),
        Subps | Subss => (
            K::Float(F::Subtract),
            4,
            instruction.mnemonic() == Subss,
            false,
        ),
        Subpd | Subsd => (
            K::Float(F::Subtract),
            8,
            instruction.mnemonic() == Subsd,
            false,
        ),
        Mulps | Mulss => (
            K::Float(F::Multiply),
            4,
            instruction.mnemonic() == Mulss,
            false,
        ),
        Mulpd | Mulsd => (
            K::Float(F::Multiply),
            8,
            instruction.mnemonic() == Mulsd,
            false,
        ),
        Divps | Divss => (
            K::Float(F::Divide),
            4,
            instruction.mnemonic() == Divss,
            false,
        ),
        Divpd | Divsd => (
            K::Float(F::Divide),
            8,
            instruction.mnemonic() == Divsd,
            false,
        ),
        Sqrtps | Sqrtss => (
            K::Float(F::SquareRoot),
            4,
            instruction.mnemonic() == Sqrtss,
            false,
        ),
        Sqrtpd | Sqrtsd => (
            K::Float(F::SquareRoot),
            8,
            instruction.mnemonic() == Sqrtsd,
            false,
        ),
        Minps | Minss => (
            K::Float(F::Minimum),
            4,
            instruction.mnemonic() == Minss,
            false,
        ),
        Minpd | Minsd => (
            K::Float(F::Minimum),
            8,
            instruction.mnemonic() == Minsd,
            false,
        ),
        Maxps | Maxss => (
            K::Float(F::Maximum),
            4,
            instruction.mnemonic() == Maxss,
            false,
        ),
        Maxpd | Maxsd => (
            K::Float(F::Maximum),
            8,
            instruction.mnemonic() == Maxsd,
            false,
        ),
        Cmpps | Cmpss => (
            K::Float(F::Compare),
            4,
            instruction.mnemonic() == Cmpss,
            false,
        ),
        Cmppd | Cmpsd => (
            K::Float(F::Compare),
            8,
            instruction.mnemonic() == Cmpsd,
            false,
        ),
        Comiss | Ucomiss => (
            K::Float(if instruction.mnemonic() == Comiss {
                F::CompareFlags
            } else {
                F::CompareFlagsQuiet
            }),
            4,
            true,
            false,
        ),
        Comisd | Ucomisd => (
            K::Float(if instruction.mnemonic() == Comisd {
                F::CompareFlags
            } else {
                F::CompareFlagsQuiet
            }),
            8,
            true,
            false,
        ),
        Rcpps | Rcpss => (
            K::Float(F::Reciprocal),
            4,
            instruction.mnemonic() == Rcpss,
            false,
        ),
        Rsqrtps | Rsqrtss => (
            K::Float(F::ReciprocalSquareRoot),
            4,
            instruction.mnemonic() == Rsqrtss,
            false,
        ),
        Cvtdq2ps => (
            K::Convert {
                kind: C::SignedDwordsToSingles,
                truncate: false,
            },
            4,
            false,
            false,
        ),
        Cvtdq2pd | Cvtpi2pd => (
            K::Convert {
                kind: C::SignedDwordsToDoubles,
                truncate: false,
            },
            8,
            false,
            false,
        ),
        Cvtpi2ps => (
            K::Convert {
                kind: C::SignedDwordsToSingles,
                truncate: false,
            },
            4,
            false,
            false,
        ),
        Cvtps2dq | Cvtps2pi | Cvttps2dq | Cvttps2pi => (
            K::Convert {
                kind: C::SinglesToSignedDwords,
                truncate: matches!(instruction.mnemonic(), Cvttps2dq | Cvttps2pi),
            },
            4,
            false,
            false,
        ),
        Cvtpd2dq | Cvtpd2pi | Cvttpd2dq | Cvttpd2pi => (
            K::Convert {
                kind: C::DoublesToSignedDwords,
                truncate: matches!(instruction.mnemonic(), Cvttpd2dq | Cvttpd2pi),
            },
            8,
            false,
            false,
        ),
        Cvtps2pd => (
            K::Convert {
                kind: C::SinglesToDoubles,
                truncate: false,
            },
            8,
            false,
            false,
        ),
        Cvtpd2ps => (
            K::Convert {
                kind: C::DoublesToSingles,
                truncate: false,
            },
            4,
            false,
            false,
        ),
        Cvtsi2ss => (
            K::Convert {
                kind: C::SignedDwordToSingle,
                truncate: false,
            },
            4,
            true,
            false,
        ),
        Cvtsi2sd => (
            K::Convert {
                kind: C::SignedDwordToDouble,
                truncate: false,
            },
            8,
            true,
            false,
        ),
        Cvtss2si | Cvttss2si => (
            K::Convert {
                kind: C::SingleToSignedDword,
                truncate: instruction.mnemonic() == Cvttss2si,
            },
            4,
            true,
            false,
        ),
        Cvtsd2si | Cvttsd2si => (
            K::Convert {
                kind: C::DoubleToSignedDword,
                truncate: instruction.mnemonic() == Cvttsd2si,
            },
            8,
            true,
            false,
        ),
        Cvtss2sd => (
            K::Convert {
                kind: C::SingleToDouble,
                truncate: false,
            },
            8,
            true,
            false,
        ),
        Cvtsd2ss => (
            K::Convert {
                kind: C::DoubleToSingle,
                truncate: false,
            },
            4,
            true,
            false,
        ),
        _ => return None,
    };
    if !matches!(
        kind,
        K::EmptyMmx
            | K::LoadMxcsr
            | K::StoreMxcsr
            | K::SaveExtended
            | K::RestoreExtended
            | K::Fence
            | K::Prefetch
    ) && instruction.mnemonic() != Movnti
        && !(0..instruction.op_count()).any(|index| {
            instruction.op_kind(index) == OpKind::Register
                && matches!(
                    instruction.op_register(index),
                    Register::XMM0
                        | Register::XMM1
                        | Register::XMM2
                        | Register::XMM3
                        | Register::XMM4
                        | Register::XMM5
                        | Register::XMM6
                        | Register::XMM7
                        | Register::MM0
                        | Register::MM1
                        | Register::MM2
                        | Register::MM3
                        | Register::MM4
                        | Register::MM5
                        | Register::MM6
                        | Register::MM7
                )
        })
    {
        return None;
    }
    Some((|| {
        let (destination, source) = match kind {
            K::EmptyMmx | K::Fence => (O::None, O::None),
            K::Prefetch => (O::None, operand(instruction, 0)?),
            K::MaskedStore => (operand(instruction, 1)?, operand(instruction, 2)?),
            K::RestoreExtended | K::LoadMxcsr => (O::None, operand(instruction, 0)?),
            K::SaveExtended | K::StoreMxcsr => (operand(instruction, 0)?, O::None),
            _ => (operand(instruction, 0)?, operand(instruction, 1)?),
        };
        let mut vector_bytes = [destination, source]
            .into_iter()
            .filter_map(|op| match op {
                O::Register(r) => Some(r.bytes()),
                _ => None,
            })
            .max()
            .unwrap_or(4);
        if instruction.mnemonic() == Movnti {
            vector_bytes = 8;
        }
        let immediate = (instruction.op_count() > 1
            && instruction.op_kind(instruction.op_count() - 1) == OpKind::Immediate8)
            .then(|| instruction.immediate8());
        let memory_bytes = if kind == K::MaskedStore {
            u16::from(vector_bytes)
        } else if matches!(destination, O::Memory(_)) || matches!(source, O::Memory(_)) {
            instruction.memory_size().size() as u16
        } else {
            0
        };
        let implicit_address = if kind == K::MaskedStore {
            Some(crate::cpu::x86::ir::EffectiveAddress {
                base: Some(crate::cpu::x86::Register32::Edi),
                address_size: if instruction.op0_kind() == OpKind::MemorySegDI {
                    crate::cpu::x86::ir::AddressSize::Bits16
                } else {
                    crate::cpu::x86::ir::AddressSize::Bits32
                },
                segment: match instruction.segment_prefix() {
                    Register::FS => Some(crate::cpu::x86::SegmentRegister::Fs),
                    Register::GS => Some(crate::cpu::x86::SegmentRegister::Gs),
                    Register::ES => Some(crate::cpu::x86::SegmentRegister::Es),
                    Register::SS => Some(crate::cpu::x86::SegmentRegister::Ss),
                    Register::CS => Some(crate::cpu::x86::SegmentRegister::Cs),
                    Register::DS => Some(crate::cpu::x86::SegmentRegister::Ds),
                    _ => None,
                },
                ..Default::default()
            })
        } else {
            None
        };
        let aligned = aligned
            || memory_bytes == 16
                && !matches!(
                    instruction.mnemonic(),
                    Movups | Movupd | Movdqu | Lddqu | Maskmovdqu | Maskmovq
                );
        Ok(SimdOperation {
            kind,
            destination,
            source,
            vector_bytes,
            lane_bytes,
            memory_bytes,
            immediate,
            aligned,
            scalar,
            implicit_address,
        })
    })())
}
