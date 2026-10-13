use super::*;
pub(super) fn lower(
    instruction: &Instruction,
    encoding: Encoding,
) -> Option<Result<AvxOperation, DecodeError>> {
    let result: Result<Option<AvxOperation>, DecodeError> = (|| {
        let (descriptor, operands, immediate) = match instruction.code() {
            Code::VEX_Vpunpcklbw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000601, operands, 0)
            }
            Code::VEX_Vpunpcklwd_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000602, operands, 0)
            }
            Code::VEX_Vpunpckldq_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000603, operands, 0)
            }
            Code::VEX_Vpacksswb_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000604, operands, 0)
            }
            Code::VEX_Vpackuswb_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000605, operands, 0)
            }
            Code::VEX_Vpunpckhbw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000606, operands, 0)
            }
            Code::VEX_Vpunpckhwd_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000607, operands, 0)
            }
            Code::VEX_Vpunpckhdq_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000608, operands, 0)
            }
            Code::VEX_Vpackssdw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000609, operands, 0)
            }
            Code::VEX_Vpunpcklqdq_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100060a, operands, 0)
            }
            Code::VEX_Vpunpckhqdq_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100060b, operands, 0)
            }
            Code::VEX_Vpshufd_xmm_xmmm128_imm8 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100060c, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpshufhw_xmm_xmmm128_imm8 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100060d, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpshuflw_xmm_xmmm128_imm8 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100060e, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpshufb_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100060f, operands, 0)
            }
            Code::VEX_Vpmovsxbw_xmm_xmmm64 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000610, operands, 0)
            }
            Code::VEX_Vpmovsxbd_xmm_xmmm32 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000611, operands, 0)
            }
            Code::VEX_Vpmovsxbq_xmm_xmmm16 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000612, operands, 0)
            }
            Code::VEX_Vpmovsxwd_xmm_xmmm64 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000613, operands, 0)
            }
            Code::VEX_Vpmovsxwq_xmm_xmmm32 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000614, operands, 0)
            }
            Code::VEX_Vpmovsxdq_xmm_xmmm64 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000615, operands, 0)
            }
            Code::VEX_Vpackusdw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000616, operands, 0)
            }
            Code::VEX_Vpmovzxbw_xmm_xmmm64 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000617, operands, 0)
            }
            Code::VEX_Vpmovzxbd_xmm_xmmm32 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000618, operands, 0)
            }
            Code::VEX_Vpmovzxbq_xmm_xmmm16 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000619, operands, 0)
            }
            Code::VEX_Vpmovzxwd_xmm_xmmm64 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100061a, operands, 0)
            }
            Code::VEX_Vpmovzxwq_xmm_xmmm32 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100061b, operands, 0)
            }
            Code::VEX_Vpmovzxdq_xmm_xmmm64 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100061c, operands, 0)
            }
            Code::VEX_Vpblendw_xmm_xmm_xmmm128_imm8 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100061d, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpalignr_xmm_xmm_xmmm128_imm8 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100061e, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpblendvb_xmm_xmm_xmmm128_xmm => {
                encoding.validate(true, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                operands |= index(instruction, 3, false, false)? << 9;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100061f, operands, 0)
            }
            Code::VEX_Vpmovsxbw_ymm_xmmm128
            | Code::VEX_Vpmovsxbd_ymm_xmmm64
            | Code::VEX_Vpmovsxbq_ymm_xmmm32
            | Code::VEX_Vpmovsxwd_ymm_xmmm128
            | Code::VEX_Vpmovsxwq_ymm_xmmm64
            | Code::VEX_Vpmovsxdq_ymm_xmmm128
            | Code::VEX_Vpmovzxbw_ymm_xmmm128
            | Code::VEX_Vpmovzxbd_ymm_xmmm64
            | Code::VEX_Vpmovzxbq_ymm_xmmm32
            | Code::VEX_Vpmovzxwd_ymm_xmmm128
            | Code::VEX_Vpmovzxwq_ymm_xmmm64
            | Code::VEX_Vpmovzxdq_ymm_xmmm128 => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                let kind = match instruction.code() {
                    Code::VEX_Vpmovsxbw_ymm_xmmm128 => 16,
                    Code::VEX_Vpmovsxbd_ymm_xmmm64 => 17,
                    Code::VEX_Vpmovsxbq_ymm_xmmm32 => 18,
                    Code::VEX_Vpmovsxwd_ymm_xmmm128 => 19,
                    Code::VEX_Vpmovsxwq_ymm_xmmm64 => 20,
                    Code::VEX_Vpmovsxdq_ymm_xmmm128 => 21,
                    Code::VEX_Vpmovzxbw_ymm_xmmm128 => 23,
                    Code::VEX_Vpmovzxbd_ymm_xmmm64 => 24,
                    Code::VEX_Vpmovzxbq_ymm_xmmm32 => 25,
                    Code::VEX_Vpmovzxwd_ymm_xmmm128 => 26,
                    Code::VEX_Vpmovzxwq_ymm_xmmm64 => 27,
                    Code::VEX_Vpmovzxdq_ymm_xmmm128 => 28,
                    _ => unreachable!("validated widening Code"),
                };
                (0x01000680 | kind, operands, 0)
            }
            _ => return Ok(None),
        };
        let instruction_value = AvxInstruction::decode_descriptor(descriptor, operands, immediate)
            .map_err(|_| invalid())?;
        let operation = bind_address(instruction, instruction_value)?;
        if descriptor & 0x80 != 0 && !crate::cpu::x86::capabilities::AVX2_ENABLED {
            return Err(DecodeError::Unsupported(UnsupportedFeature::Simd));
        }
        Ok(Some(operation))
    })();
    result.transpose()
}
