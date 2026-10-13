use super::*;
pub(super) fn lower(
    instruction: &Instruction,
    encoding: Encoding,
) -> Option<Result<AvxOperation, DecodeError>> {
    let result: Result<Option<AvxOperation>, DecodeError> = (|| {
        let (descriptor, operands, immediate) = match instruction.code() {
            Code::VEX_Vucomiss_xmm_xmmm32 => {
                encoding.validate(false, 3, true)?;
                let mut operands = index(instruction, 0, false, false)? << 3;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000a01, operands, 0)
            }
            Code::VEX_Vucomisd_xmm_xmmm64 => {
                encoding.validate(false, 3, true)?;
                let mut operands = index(instruction, 0, false, false)? << 3;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000a02, operands, 0)
            }
            Code::VEX_Vcomiss_xmm_xmmm32 => {
                encoding.validate(false, 3, true)?;
                let mut operands = index(instruction, 0, false, false)? << 3;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000a03, operands, 0)
            }
            Code::VEX_Vcomisd_xmm_xmmm64 => {
                encoding.validate(false, 3, true)?;
                let mut operands = index(instruction, 0, false, false)? << 3;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000a04, operands, 0)
            }
            Code::VEX_Vcmpps_xmm_xmm_xmmm128_imm8 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000a05, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vcmpps_ymm_ymm_ymmm256_imm8 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000a06, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vcmppd_xmm_xmm_xmmm128_imm8 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000a07, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vcmppd_ymm_ymm_ymmm256_imm8 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000a08, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vcmpss_xmm_xmm_xmmm32_imm8 => {
                encoding.validate(false, 3, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000a09, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vcmpsd_xmm_xmm_xmmm64_imm8 => {
                encoding.validate(false, 3, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000a0a, operands, u32::from(instruction.immediate8()))
            }
            _ => return Ok(None),
        };
        let instruction_value = AvxInstruction::decode_descriptor(descriptor, operands, immediate)
            .map_err(|_| invalid())?;
        bind_address(instruction, instruction_value).map(Some)
    })();
    result.transpose()
}
