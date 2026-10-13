use super::*;
pub(super) fn lower(
    instruction: &Instruction,
    encoding: Encoding,
) -> Option<Result<AvxOperation, DecodeError>> {
    let result: Result<Option<AvxOperation>, DecodeError> = (|| {
        let (descriptor, operands, immediate) = match instruction.code() {
            Code::VEX_Vtestps_xmm_xmmm128 => {
                encoding.validate(true, 0, true)?;
                let mut operands = index(instruction, 0, false, false)? << 3;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000f01, operands, 0)
            }
            Code::VEX_Vtestps_ymm_ymmm256 => {
                encoding.validate(true, 1, true)?;
                let mut operands = index(instruction, 0, false, false)? << 3;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000f02, operands, 0)
            }
            Code::VEX_Vtestpd_xmm_xmmm128 => {
                encoding.validate(true, 0, true)?;
                let mut operands = index(instruction, 0, false, false)? << 3;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000f03, operands, 0)
            }
            Code::VEX_Vtestpd_ymm_ymmm256 => {
                encoding.validate(true, 1, true)?;
                let mut operands = index(instruction, 0, false, false)? << 3;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000f04, operands, 0)
            }
            Code::VEX_Vptest_xmm_xmmm128 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)? << 3;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000f05, operands, 0)
            }
            Code::VEX_Vptest_ymm_ymmm256 => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, false)? << 3;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000f06, operands, 0)
            }
            _ => return Ok(None),
        };
        let instruction_value = AvxInstruction::decode_descriptor(descriptor, operands, immediate)
            .map_err(|_| invalid())?;
        bind_address(instruction, instruction_value).map(Some)
    })();
    result.transpose()
}
