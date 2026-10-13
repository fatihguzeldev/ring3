use super::*;
use crate::cpu::x86::avx::AvxSemantic;
pub(super) fn lower(
    instruction: &Instruction,
    encoding: Encoding,
) -> Option<Result<AvxOperation, DecodeError>> {
    let result: Result<Option<AvxOperation>, DecodeError> = (|| {
        let (descriptor, operands, immediate) = match instruction.code() {
            Code::VEX_Vpsrlw_xmm_xmm_imm8 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                (0x01000701, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpsraw_xmm_xmm_imm8 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                (0x01000702, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpsllw_xmm_xmm_imm8 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                (0x01000703, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpsrld_xmm_xmm_imm8 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                (0x01000704, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpsrad_xmm_xmm_imm8 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                (0x01000705, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpslld_xmm_xmm_imm8 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                (0x01000706, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpsrlq_xmm_xmm_imm8 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                (0x01000707, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpsrldq_xmm_xmm_imm8 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                (0x01000708, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpsllq_xmm_xmm_imm8 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                (0x01000709, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpslldq_xmm_xmm_imm8 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                (0x0100070a, operands, u32::from(instruction.immediate8()))
            }
            Code::VEX_Vpsrlw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100070b, operands, 0)
            }
            Code::VEX_Vpsrld_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100070c, operands, 0)
            }
            Code::VEX_Vpsrlq_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100070d, operands, 0)
            }
            Code::VEX_Vpsraw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100070e, operands, 0)
            }
            Code::VEX_Vpsrad_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100070f, operands, 0)
            }
            Code::VEX_Vpsllw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000710, operands, 0)
            }
            Code::VEX_Vpslld_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000711, operands, 0)
            }
            Code::VEX_Vpsllq_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000712, operands, 0)
            }
            Code::VEX_Vpsrlvd_xmm_xmm_xmmm128
            | Code::VEX_Vpsrlvd_ymm_ymm_ymmm256
            | Code::VEX_Vpsrlvq_xmm_xmm_xmmm128
            | Code::VEX_Vpsrlvq_ymm_ymm_ymmm256
            | Code::VEX_Vpsravd_xmm_xmm_xmmm128
            | Code::VEX_Vpsravd_ymm_ymm_ymmm256
            | Code::VEX_Vpsllvd_xmm_xmm_xmmm128
            | Code::VEX_Vpsllvd_ymm_ymm_ymmm256
            | Code::VEX_Vpsllvq_xmm_xmm_xmmm128
            | Code::VEX_Vpsllvq_ymm_ymm_ymmm256 => {
                let (kind, wide, w) = match instruction.code() {
                    Code::VEX_Vpsrlvd_xmm_xmm_xmmm128 => (19, false, false),
                    Code::VEX_Vpsrlvd_ymm_ymm_ymmm256 => (19, true, false),
                    Code::VEX_Vpsrlvq_xmm_xmm_xmmm128 => (20, false, true),
                    Code::VEX_Vpsrlvq_ymm_ymm_ymmm256 => (20, true, true),
                    Code::VEX_Vpsravd_xmm_xmm_xmmm128 => (21, false, false),
                    Code::VEX_Vpsravd_ymm_ymm_ymmm256 => (21, true, false),
                    Code::VEX_Vpsllvd_xmm_xmm_xmmm128 => (22, false, false),
                    Code::VEX_Vpsllvd_ymm_ymm_ymmm256 => (22, true, false),
                    Code::VEX_Vpsllvq_xmm_xmm_xmmm128 => (23, false, true),
                    Code::VEX_Vpsllvq_ymm_ymm_ymmm256 => (23, true, true),
                    _ => unreachable!("validated variable shift Code"),
                };
                encoding.validate_w(w)?;
                encoding.validate(false, u8::from(wide), false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000700 | kind | if wide { 0x80 } else { 0 }, operands, 0)
            }
            _ => {
                let (kind, immediate_count) = match instruction.code() {
                    Code::VEX_Vpsrlw_ymm_ymm_imm8 => (1, true),
                    Code::VEX_Vpsraw_ymm_ymm_imm8 => (2, true),
                    Code::VEX_Vpsllw_ymm_ymm_imm8 => (3, true),
                    Code::VEX_Vpsrld_ymm_ymm_imm8 => (4, true),
                    Code::VEX_Vpsrad_ymm_ymm_imm8 => (5, true),
                    Code::VEX_Vpslld_ymm_ymm_imm8 => (6, true),
                    Code::VEX_Vpsrlq_ymm_ymm_imm8 => (7, true),
                    Code::VEX_Vpsrldq_ymm_ymm_imm8 => (8, true),
                    Code::VEX_Vpsllq_ymm_ymm_imm8 => (9, true),
                    Code::VEX_Vpslldq_ymm_ymm_imm8 => (10, true),
                    Code::VEX_Vpsrlw_ymm_ymm_xmmm128 => (11, false),
                    Code::VEX_Vpsrld_ymm_ymm_xmmm128 => (12, false),
                    Code::VEX_Vpsrlq_ymm_ymm_xmmm128 => (13, false),
                    Code::VEX_Vpsraw_ymm_ymm_xmmm128 => (14, false),
                    Code::VEX_Vpsrad_ymm_ymm_xmmm128 => (15, false),
                    Code::VEX_Vpsllw_ymm_ymm_xmmm128 => (16, false),
                    Code::VEX_Vpslld_ymm_ymm_xmmm128 => (17, false),
                    Code::VEX_Vpsllq_ymm_ymm_xmmm128 => (18, false),
                    _ => return Ok(None),
                };
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                let immediate = if immediate_count {
                    u32::from(instruction.immediate8())
                } else {
                    operands |= index(instruction, 2, false, true)? << 6;
                    if instruction.op_kind(2) == OpKind::Memory {
                        operands |= 1 << 12;
                    }
                    0
                };
                (0x01000780 | kind, operands, immediate)
            }
        };
        let instruction_value = AvxInstruction::decode_descriptor(descriptor, operands, immediate)
            .map_err(|_| invalid())?;
        let operation = bind_address(instruction, instruction_value)?;
        let AvxSemantic::Shift(value) = instruction_value.semantic() else {
            unreachable!("shift decoder constructs only shift instructions")
        };
        if !crate::cpu::x86::capabilities::AVX2_ENABLED && value.requires_avx2() {
            return Err(unsupported());
        }
        Ok(Some(operation))
    })();
    result.transpose()
}
