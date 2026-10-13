use super::*;
pub(super) fn lower(
    instruction: &Instruction,
    encoding: Encoding,
) -> Option<Result<AvxOperation, DecodeError>> {
    let result: Result<Option<AvxOperation>, DecodeError> = (|| {
        let (descriptor, operands, immediate) = match instruction.code() {
            Code::VEX_Vhaddpd_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000c01, operands, 0)
            }
            Code::VEX_Vhaddpd_ymm_ymm_ymmm256 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000c02, operands, 0)
            }
            Code::VEX_Vhaddps_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000c03, operands, 0)
            }
            Code::VEX_Vhaddps_ymm_ymm_ymmm256 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000c04, operands, 0)
            }
            Code::VEX_Vhsubpd_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000c05, operands, 0)
            }
            Code::VEX_Vhsubpd_ymm_ymm_ymmm256 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000c06, operands, 0)
            }
            Code::VEX_Vhsubps_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000c07, operands, 0)
            }
            Code::VEX_Vhsubps_ymm_ymm_ymmm256 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000c08, operands, 0)
            }
            Code::VEX_Vaddsubpd_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000c09, operands, 0)
            }
            Code::VEX_Vaddsubpd_ymm_ymm_ymmm256 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000c0a, operands, 0)
            }
            Code::VEX_Vaddsubps_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000c0b, operands, 0)
            }
            Code::VEX_Vaddsubps_ymm_ymm_ymmm256 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000c0c, operands, 0)
            }
            Code::VEX_Vroundps_xmm_xmmm128_imm8 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000c0d, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vroundps_ymm_ymmm256_imm8 => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000c0e, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vroundpd_xmm_xmmm128_imm8 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000c0f, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vroundpd_ymm_ymmm256_imm8 => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000c10, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vroundss_xmm_xmm_xmmm32_imm8 => {
                encoding.validate(false, 2, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000c11, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vroundsd_xmm_xmm_xmmm64_imm8 => {
                encoding.validate(false, 2, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000c12, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vdpps_xmm_xmm_xmmm128_imm8 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000c13, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vdpps_ymm_ymm_ymmm256_imm8 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000c14, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vdppd_xmm_xmm_xmmm128_imm8 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000c15, operands, u32::from(instruction.immediate8()))
            }
            _ => return Ok(None),
        };
        let instruction_value = AvxInstruction::decode_descriptor(descriptor, operands, immediate)
            .map_err(|_| invalid())?;
        bind_address(instruction, instruction_value).map(Some)
    })();
    result.transpose()
}
