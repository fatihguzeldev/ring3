use super::*;
use crate::cpu::x86::avx::{AvxSemantic, VectorWidth};
pub(super) fn lower(
    instruction: &Instruction,
    encoding: Encoding,
) -> Option<Result<AvxOperation, DecodeError>> {
    let result: Result<Option<AvxOperation>, DecodeError> = (|| {
        let (descriptor, operands, immediate) = match instruction.code() {
            Code::VEX_Vpmullw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000801, operands, 0)
            }
            Code::VEX_Vpmulhuw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000802, operands, 0)
            }
            Code::VEX_Vpmulhw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000803, operands, 0)
            }
            Code::VEX_Vpmuludq_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000804, operands, 0)
            }
            Code::VEX_Vpmaddwd_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000805, operands, 0)
            }
            Code::VEX_Vpsadbw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000806, operands, 0)
            }
            Code::VEX_Vphaddw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000807, operands, 0)
            }
            Code::VEX_Vphaddd_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000808, operands, 0)
            }
            Code::VEX_Vphaddsw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000809, operands, 0)
            }
            Code::VEX_Vpmaddubsw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100080a, operands, 0)
            }
            Code::VEX_Vphsubw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100080b, operands, 0)
            }
            Code::VEX_Vphsubd_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100080c, operands, 0)
            }
            Code::VEX_Vphsubsw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100080d, operands, 0)
            }
            Code::VEX_Vpmulhrsw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100080e, operands, 0)
            }
            Code::VEX_Vpmuldq_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100080f, operands, 0)
            }
            Code::VEX_Vpmulld_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000810, operands, 0)
            }
            Code::VEX_Vphminposuw_xmm_xmmm128 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000811, operands, 0)
            }
            Code::VEX_Vmpsadbw_xmm_xmm_xmmm128_imm8 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000812, operands, u32::from(instruction.immediate8()))
            }
            _ => {
                let kind = match instruction.code() {
                    Code::VEX_Vpmullw_ymm_ymm_ymmm256 => 1,
                    Code::VEX_Vpmulhuw_ymm_ymm_ymmm256 => 2,
                    Code::VEX_Vpmulhw_ymm_ymm_ymmm256 => 3,
                    Code::VEX_Vpmuludq_ymm_ymm_ymmm256 => 4,
                    Code::VEX_Vpmaddwd_ymm_ymm_ymmm256 => 5,
                    Code::VEX_Vpsadbw_ymm_ymm_ymmm256 => 6,
                    Code::VEX_Vphaddw_ymm_ymm_ymmm256 => 7,
                    Code::VEX_Vphaddd_ymm_ymm_ymmm256 => 8,
                    Code::VEX_Vphaddsw_ymm_ymm_ymmm256 => 9,
                    Code::VEX_Vpmaddubsw_ymm_ymm_ymmm256 => 10,
                    Code::VEX_Vphsubw_ymm_ymm_ymmm256 => 11,
                    Code::VEX_Vphsubd_ymm_ymm_ymmm256 => 12,
                    Code::VEX_Vphsubsw_ymm_ymm_ymmm256 => 13,
                    Code::VEX_Vpmulhrsw_ymm_ymm_ymmm256 => 14,
                    Code::VEX_Vpmuldq_ymm_ymm_ymmm256 => 15,
                    Code::VEX_Vpmulld_ymm_ymm_ymmm256 => 16,
                    _ => return Ok(None),
                };
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000880 | kind, operands, 0)
            }
        };
        let instruction_value = AvxInstruction::decode_descriptor(descriptor, operands, immediate)
            .map_err(|_| invalid())?;
        let operation = bind_address(instruction, instruction_value)?;
        let AvxSemantic::Multiply(value) = instruction_value.semantic() else {
            unreachable!("multiply decoder constructs only multiply instructions")
        };
        if !crate::cpu::x86::capabilities::AVX2_ENABLED && value.width() == VectorWidth::Ymm {
            return Err(unsupported());
        }
        Ok(Some(operation))
    })();
    result.transpose()
}
