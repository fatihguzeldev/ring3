use super::*;
pub(super) fn lower(
    instruction: &Instruction,
    encoding: Encoding,
) -> Option<Result<AvxOperation, DecodeError>> {
    let result: Result<Option<AvxOperation>, DecodeError> = (|| {
        let (descriptor, operands, immediate) = match instruction.code() {
            Code::VEX_Vmaskmovdqu_rDI_xmm_xmm => {
                encoding.validate(false, 0, true)?;
                if !matches!(
                    instruction.op0_kind(),
                    OpKind::MemorySegDI | OpKind::MemorySegEDI
                ) {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 1, false, false)? << 6;
                operands |= index(instruction, 2, false, false)? << 9;
                if true {
                    operands |= 1 << 13;
                }
                (0x01000d01, operands, 0)
            }
            Code::VEX_Vmaskmovps_xmm_xmm_m128 => {
                encoding.validate(true, 0, false)?;
                if instruction.op_kind(2) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 9;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000d02, operands, 0)
            }
            Code::VEX_Vmaskmovps_ymm_ymm_m256 => {
                encoding.validate(true, 1, false)?;
                if instruction.op_kind(2) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 9;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000d03, operands, 0)
            }
            Code::VEX_Vmaskmovpd_xmm_xmm_m128 => {
                encoding.validate(true, 0, false)?;
                if instruction.op_kind(2) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 9;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000d04, operands, 0)
            }
            Code::VEX_Vmaskmovpd_ymm_ymm_m256 => {
                encoding.validate(true, 1, false)?;
                if instruction.op_kind(2) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 9;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000d05, operands, 0)
            }
            Code::VEX_Vmaskmovps_m128_xmm_xmm => {
                encoding.validate(true, 0, false)?;
                if instruction.op_kind(0) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 2, false, false)? << 6;
                operands |= index(instruction, 1, false, false)? << 9;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x01000d06, operands, 0)
            }
            Code::VEX_Vmaskmovps_m256_ymm_ymm => {
                encoding.validate(true, 1, false)?;
                if instruction.op_kind(0) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 2, false, false)? << 6;
                operands |= index(instruction, 1, false, false)? << 9;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x01000d07, operands, 0)
            }
            Code::VEX_Vmaskmovpd_m128_xmm_xmm => {
                encoding.validate(true, 0, false)?;
                if instruction.op_kind(0) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 2, false, false)? << 6;
                operands |= index(instruction, 1, false, false)? << 9;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x01000d08, operands, 0)
            }
            Code::VEX_Vmaskmovpd_m256_ymm_ymm => {
                encoding.validate(true, 1, false)?;
                if instruction.op_kind(0) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 2, false, false)? << 6;
                operands |= index(instruction, 1, false, false)? << 9;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x01000d09, operands, 0)
            }
            _ => return Ok(None),
        };
        let instruction_value = AvxInstruction::decode_descriptor(descriptor, operands, immediate)
            .map_err(|_| invalid())?;
        bind_address(instruction, instruction_value).map(Some)
    })();
    result.transpose()
}
