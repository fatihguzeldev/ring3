use super::*;
pub(super) fn lower(
    instruction: &Instruction,
    encoding: Encoding,
) -> Option<Result<AvxOperation, DecodeError>> {
    let result: Result<Option<AvxOperation>, DecodeError> = (|| {
        let (descriptor, operands, immediate) = match instruction.code() {
            Code::VEX_Vpinsrw_xmm_xmm_r32m16_imm8 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, true, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000901, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpextrw_r32_xmm_imm8 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, true, false)?;
                operands |= index(instruction, 1, false, false)? << 6;
                (0x01000902, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpmovmskb_r32_xmm | Code::VEX_Vpmovmskb_r32_ymm => {
                let wide = instruction.code() == Code::VEX_Vpmovmskb_r32_ymm;
                encoding.validate(false, u8::from(wide), true)?;
                let mut operands = index(instruction, 0, true, false)?;
                operands |= index(instruction, 1, false, false)? << 6;
                (if wide { 0x01000983 } else { 0x01000903 }, operands, 0)
            }
            Code::VEX_Vpextrb_r32m8_xmm_imm8 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, true, true)?;
                operands |= index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x01000904, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpextrw_r32m16_xmm_imm8 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, true, true)?;
                operands |= index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x01000905, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpextrd_rm32_xmm_imm8 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, true, true)?;
                operands |= index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x01000906, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpinsrb_xmm_xmm_r32m8_imm8 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, true, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000907, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpinsrd_xmm_xmm_rm32_imm8 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, true, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000908, operands, u32::from(instruction.immediate8()))
            }
            _ => return Ok(None),
        };
        let instruction_value = AvxInstruction::decode_descriptor(descriptor, operands, immediate)
            .map_err(|_| invalid())?;
        let operation = bind_address(instruction, instruction_value)?;
        if instruction.code() == Code::VEX_Vpmovmskb_r32_ymm
            && !crate::cpu::x86::capabilities::AVX2_ENABLED
        {
            return Err(unsupported());
        }
        Ok(Some(operation))
    })();
    result.transpose()
}
