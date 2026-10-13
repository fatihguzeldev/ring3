use super::*;
pub(super) fn lower(
    instruction: &Instruction,
    encoding: Encoding,
) -> Option<Result<AvxOperation, DecodeError>> {
    let result: Result<Option<AvxOperation>, DecodeError> = (|| {
        let (descriptor, operands, immediate) = match instruction.code() {
            Code::VEX_Vbroadcastss_xmm_m32 => {
                encoding.validate(true, 0, true)?;
                if instruction.op_kind(1) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 0, false, false)?;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000e01, operands, 0)
            }
            Code::VEX_Vbroadcastss_ymm_m32 => {
                encoding.validate(true, 1, true)?;
                if instruction.op_kind(1) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 0, false, false)?;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000e02, operands, 0)
            }
            Code::VEX_Vbroadcastsd_ymm_m64 => {
                encoding.validate(true, 1, true)?;
                if instruction.op_kind(1) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 0, false, false)?;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000e03, operands, 0)
            }
            Code::VEX_Vpbroadcastb_xmm_xmmm8
            | Code::VEX_Vpbroadcastb_ymm_xmmm8
            | Code::VEX_Vpbroadcastw_xmm_xmmm16
            | Code::VEX_Vpbroadcastw_ymm_xmmm16
            | Code::VEX_Vpbroadcastd_xmm_xmmm32
            | Code::VEX_Vpbroadcastd_ymm_xmmm32
            | Code::VEX_Vpbroadcastq_xmm_xmmm64
            | Code::VEX_Vpbroadcastq_ymm_xmmm64
            | Code::VEX_Vbroadcastss_xmm_xmm
            | Code::VEX_Vbroadcastss_ymm_xmm
            | Code::VEX_Vbroadcastsd_ymm_xmm => {
                let (register_kind, memory_kind, width) = match instruction.code() {
                    Code::VEX_Vpbroadcastb_xmm_xmmm8 => (8, 8, 0),
                    Code::VEX_Vpbroadcastb_ymm_xmmm8 => (9, 9, 1),
                    Code::VEX_Vpbroadcastw_xmm_xmmm16 => (10, 10, 0),
                    Code::VEX_Vpbroadcastw_ymm_xmmm16 => (11, 11, 1),
                    Code::VEX_Vpbroadcastd_xmm_xmmm32 | Code::VEX_Vbroadcastss_xmm_xmm => {
                        (13, 1, 0)
                    }
                    Code::VEX_Vpbroadcastd_ymm_xmmm32 | Code::VEX_Vbroadcastss_ymm_xmm => {
                        (14, 2, 1)
                    }
                    Code::VEX_Vpbroadcastq_xmm_xmmm64 => (12, 12, 0),
                    Code::VEX_Vpbroadcastq_ymm_xmmm64 | Code::VEX_Vbroadcastsd_ymm_xmm => {
                        (15, 3, 1)
                    }
                    _ => unreachable!("closed scalar broadcast codes"),
                };
                encoding.validate(true, width, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                let kind = if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                    memory_kind
                } else {
                    register_kind
                };
                (0x01000e00 | kind, operands, 0)
            }
            Code::VEX_Vbroadcastf128_ymm_m128 | Code::VEX_Vbroadcasti128_ymm_m128 => {
                encoding.validate(true, 1, true)?;
                if instruction.op_kind(1) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 0, false, false)?;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000e04, operands, 0)
            }
            Code::VEX_Vperm2f128_ymm_ymm_ymmm256_imm8
            | Code::VEX_Vperm2i128_ymm_ymm_ymmm256_imm8 => {
                encoding.validate(true, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000e05, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vinsertf128_ymm_ymm_xmmm128_imm8
            | Code::VEX_Vinserti128_ymm_ymm_xmmm128_imm8 => {
                encoding.validate(true, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000e06, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vextractf128_xmmm128_ymm_imm8 | Code::VEX_Vextracti128_xmmm128_ymm_imm8 => {
                encoding.validate(true, 1, true)?;
                let mut operands = index(instruction, 0, false, true)?;
                operands |= index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x01000e07, operands, u32::from(instruction.immediate8()))
            }
            _ => return Ok(None),
        };
        let instruction_value = AvxInstruction::decode_descriptor(descriptor, operands, immediate)
            .map_err(|_| invalid())?;
        let operation = bind_address(instruction, instruction_value)?;
        if !crate::cpu::x86::capabilities::AVX2_ENABLED
            && matches!(
                instruction.code(),
                Code::VEX_Vpbroadcastb_xmm_xmmm8
                    | Code::VEX_Vpbroadcastb_ymm_xmmm8
                    | Code::VEX_Vpbroadcastw_xmm_xmmm16
                    | Code::VEX_Vpbroadcastw_ymm_xmmm16
                    | Code::VEX_Vpbroadcastd_xmm_xmmm32
                    | Code::VEX_Vpbroadcastd_ymm_xmmm32
                    | Code::VEX_Vpbroadcastq_xmm_xmmm64
                    | Code::VEX_Vpbroadcastq_ymm_xmmm64
                    | Code::VEX_Vbroadcastss_xmm_xmm
                    | Code::VEX_Vbroadcastss_ymm_xmm
                    | Code::VEX_Vbroadcastsd_ymm_xmm
                    | Code::VEX_Vbroadcasti128_ymm_m128
                    | Code::VEX_Vperm2i128_ymm_ymm_ymmm256_imm8
                    | Code::VEX_Vinserti128_ymm_ymm_xmmm128_imm8
                    | Code::VEX_Vextracti128_xmmm128_ymm_imm8
            )
        {
            return Err(unsupported());
        }
        Ok(Some(operation))
    })();
    result.transpose()
}
