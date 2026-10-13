use super::*;
pub(super) fn lower(
    instruction: &Instruction,
    encoding: Encoding,
) -> Option<Result<AvxOperation, DecodeError>> {
    let result: Result<Option<AvxOperation>, DecodeError> = (|| {
        let (descriptor, operands, immediate) = match instruction.code() {
            Code::VEX_Vpcmpestrm_xmm_xmmm128_imm8 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)? << 3;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01001101, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpcmpestri_xmm_xmmm128_imm8 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)? << 3;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01001102, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpcmpistrm_xmm_xmmm128_imm8 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)? << 3;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01001103, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpcmpistri_xmm_xmmm128_imm8 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)? << 3;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01001104, operands, u32::from(instruction.immediate8()))
            }
            _ => return Ok(None),
        };
        let instruction_value = AvxInstruction::decode_descriptor(descriptor, operands, immediate)
            .map_err(|_| invalid())?;
        bind_address(instruction, instruction_value).map(Some)
    })();
    result.transpose()
}
