use super::*;
pub(super) fn lower(
    instruction: &Instruction,
    encoding: Encoding,
) -> Option<Result<AvxOperation, DecodeError>> {
    let result: Result<Option<AvxOperation>, DecodeError> = (|| {
        let (descriptor, operands, immediate) = match instruction.code() {
            Code::VEX_Vcvtsi2ss_xmm_xmm_rm32 => {
                encoding.validate(false, 3, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, true, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000b01, operands, 0)
            }
            Code::VEX_Vcvtsi2sd_xmm_xmm_rm32 => {
                encoding.validate(false, 3, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, true, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000b02, operands, 0)
            }
            Code::VEX_Vcvttss2si_r32_xmmm32 => {
                encoding.validate(false, 3, true)?;
                let mut operands = index(instruction, 0, true, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000b03, operands, 0)
            }
            Code::VEX_Vcvttsd2si_r32_xmmm64 => {
                encoding.validate(false, 3, true)?;
                let mut operands = index(instruction, 0, true, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000b04, operands, 0)
            }
            Code::VEX_Vcvtss2si_r32_xmmm32 => {
                encoding.validate(false, 3, true)?;
                let mut operands = index(instruction, 0, true, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000b05, operands, 0)
            }
            Code::VEX_Vcvtsd2si_r32_xmmm64 => {
                encoding.validate(false, 3, true)?;
                let mut operands = index(instruction, 0, true, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000b06, operands, 0)
            }
            Code::VEX_Vcvtps2pd_xmm_xmmm64 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000b07, operands, 0)
            }
            Code::VEX_Vcvtps2pd_ymm_xmmm128 => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000b08, operands, 0)
            }
            Code::VEX_Vcvtpd2ps_xmm_xmmm128 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000b09, operands, 0)
            }
            Code::VEX_Vcvtpd2ps_xmm_ymmm256 => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000b0a, operands, 0)
            }
            Code::VEX_Vcvtss2sd_xmm_xmm_xmmm32 => {
                encoding.validate(false, 3, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000b0b, operands, 0)
            }
            Code::VEX_Vcvtsd2ss_xmm_xmm_xmmm64 => {
                encoding.validate(false, 3, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000b0c, operands, 0)
            }
            Code::VEX_Vcvtdq2ps_xmm_xmmm128 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000b0d, operands, 0)
            }
            Code::VEX_Vcvtdq2ps_ymm_ymmm256 => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000b0e, operands, 0)
            }
            Code::VEX_Vcvtps2dq_xmm_xmmm128 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000b0f, operands, 0)
            }
            Code::VEX_Vcvtps2dq_ymm_ymmm256 => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000b10, operands, 0)
            }
            Code::VEX_Vcvttps2dq_xmm_xmmm128 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000b11, operands, 0)
            }
            Code::VEX_Vcvttps2dq_ymm_ymmm256 => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000b12, operands, 0)
            }
            Code::VEX_Vcvttpd2dq_xmm_xmmm128 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000b13, operands, 0)
            }
            Code::VEX_Vcvttpd2dq_xmm_ymmm256 => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000b14, operands, 0)
            }
            Code::VEX_Vcvtdq2pd_xmm_xmmm64 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000b15, operands, 0)
            }
            Code::VEX_Vcvtdq2pd_ymm_xmmm128 => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000b16, operands, 0)
            }
            Code::VEX_Vcvtpd2dq_xmm_xmmm128 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000b17, operands, 0)
            }
            Code::VEX_Vcvtpd2dq_xmm_ymmm256 => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000b18, operands, 0)
            }
            _ => return Ok(None),
        };
        let instruction_value = AvxInstruction::decode_descriptor(descriptor, operands, immediate)
            .map_err(|_| invalid())?;
        bind_address(instruction, instruction_value).map(Some)
    })();
    result.transpose()
}
