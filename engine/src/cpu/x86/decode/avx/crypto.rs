use super::*;
pub(super) fn lower(
    instruction: &Instruction,
    encoding: Encoding,
) -> Option<Result<AvxOperation, DecodeError>> {
    let result: Result<Option<AvxOperation>, DecodeError> = (|| {
        let (descriptor, operands, immediate) = match instruction.code() {
            Code::VEX_Vaesimc_xmm_xmmm128 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01001001, operands, 0)
            }
            Code::VEX_Vaesenc_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01001002, operands, 0)
            }
            Code::VEX_Vaesenclast_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01001003, operands, 0)
            }
            Code::VEX_Vaesdec_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01001004, operands, 0)
            }
            Code::VEX_Vaesdeclast_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01001005, operands, 0)
            }
            Code::VEX_Vpclmulqdq_xmm_xmm_xmmm128_imm8 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01001006, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vaeskeygenassist_xmm_xmmm128_imm8 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01001007, operands, u32::from(instruction.immediate8()))
            }
            _ => return Ok(None),
        };
        let instruction_value = AvxInstruction::decode_descriptor(descriptor, operands, immediate)
            .map_err(|_| invalid())?;
        bind_address(instruction, instruction_value).map(Some)
    })();
    result.transpose()
}
