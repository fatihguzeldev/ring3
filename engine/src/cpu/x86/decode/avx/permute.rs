use super::*;
pub(super) fn lower(
    instruction: &Instruction,
    encoding: Encoding,
) -> Option<Result<AvxOperation, DecodeError>> {
    let result: Result<Option<AvxOperation>, DecodeError> = (|| {
        let (descriptor, operands, immediate) = match instruction.code() {
            Code::VEX_Vunpcklps_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000401, operands, 0)
            }
            Code::VEX_Vunpcklps_ymm_ymm_ymmm256 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000402, operands, 0)
            }
            Code::VEX_Vunpcklpd_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000403, operands, 0)
            }
            Code::VEX_Vunpcklpd_ymm_ymm_ymmm256 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000404, operands, 0)
            }
            Code::VEX_Vunpckhps_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000405, operands, 0)
            }
            Code::VEX_Vunpckhps_ymm_ymm_ymmm256 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000406, operands, 0)
            }
            Code::VEX_Vunpckhpd_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000407, operands, 0)
            }
            Code::VEX_Vunpckhpd_ymm_ymm_ymmm256 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000408, operands, 0)
            }
            Code::VEX_Vshufps_xmm_xmm_xmmm128_imm8 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000409, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vshufps_ymm_ymm_ymmm256_imm8 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100040a, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vshufpd_xmm_xmm_xmmm128_imm8 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100040b, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vshufpd_ymm_ymm_ymmm256_imm8 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100040c, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpermilps_xmm_xmm_xmmm128 => {
                encoding.validate(true, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100040d, operands, 0)
            }
            Code::VEX_Vpermilps_ymm_ymm_ymmm256 => {
                encoding.validate(true, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100040e, operands, 0)
            }
            Code::VEX_Vpermilpd_xmm_xmm_xmmm128 => {
                encoding.validate(true, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100040f, operands, 0)
            }
            Code::VEX_Vpermilpd_ymm_ymm_ymmm256 => {
                encoding.validate(true, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000410, operands, 0)
            }
            Code::VEX_Vpermilps_xmm_xmmm128_imm8 => {
                encoding.validate(true, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000411, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpermilps_ymm_ymmm256_imm8 => {
                encoding.validate(true, 1, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000412, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpermilpd_xmm_xmmm128_imm8 => {
                encoding.validate(true, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000413, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpermilpd_ymm_ymmm256_imm8 => {
                encoding.validate(true, 1, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000414, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vblendps_xmm_xmm_xmmm128_imm8 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000415, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vblendps_ymm_ymm_ymmm256_imm8 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000416, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vblendpd_xmm_xmm_xmmm128_imm8 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000417, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vblendpd_ymm_ymm_ymmm256_imm8 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000418, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vextractps_rm32_xmm_imm8 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, true, true)?;
                operands |= index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x01000419, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vinsertps_xmm_xmm_xmmm32_imm8 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100041a, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vblendvps_xmm_xmm_xmmm128_xmm => {
                encoding.validate(true, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                operands |= index(instruction, 3, false, false)? << 9;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100041b, operands, 0)
            }
            Code::VEX_Vblendvps_ymm_ymm_ymmm256_ymm => {
                encoding.validate(true, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                operands |= index(instruction, 3, false, false)? << 9;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100041c, operands, 0)
            }
            Code::VEX_Vblendvpd_xmm_xmm_xmmm128_xmm => {
                encoding.validate(true, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                operands |= index(instruction, 3, false, false)? << 9;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100041d, operands, 0)
            }
            Code::VEX_Vblendvpd_ymm_ymm_ymmm256_ymm => {
                encoding.validate(true, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                operands |= index(instruction, 3, false, false)? << 9;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100041e, operands, 0)
            }
            _ => return Ok(None),
        };
        let instruction_value = AvxInstruction::decode_descriptor(descriptor, operands, immediate)
            .map_err(|_| invalid())?;
        bind_address(instruction, instruction_value).map(Some)
    })();
    result.transpose()
}
