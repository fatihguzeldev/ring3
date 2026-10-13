use super::*;
pub(super) fn lower(
    instruction: &Instruction,
    encoding: Encoding,
) -> Option<Result<AvxOperation, DecodeError>> {
    let result: Result<Option<AvxOperation>, DecodeError> = (|| {
        let (descriptor, operands, immediate) = match instruction.code() {
            Code::VEX_Vsqrtps_xmm_xmmm128 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000101, operands, 0)
            }
            Code::VEX_Vsqrtps_ymm_ymmm256 => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000102, operands, 0)
            }
            Code::VEX_Vsqrtpd_xmm_xmmm128 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000103, operands, 0)
            }
            Code::VEX_Vsqrtpd_ymm_ymmm256 => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000104, operands, 0)
            }
            Code::VEX_Vsqrtss_xmm_xmm_xmmm32 => {
                encoding.validate(false, 3, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000105, operands, 0)
            }
            Code::VEX_Vsqrtsd_xmm_xmm_xmmm64 => {
                encoding.validate(false, 3, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000106, operands, 0)
            }
            Code::VEX_Vrsqrtps_xmm_xmmm128 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000107, operands, 0)
            }
            Code::VEX_Vrsqrtps_ymm_ymmm256 => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000108, operands, 0)
            }
            Code::VEX_Vrsqrtss_xmm_xmm_xmmm32 => {
                encoding.validate(false, 2, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000109, operands, 0)
            }
            Code::VEX_Vrcpps_xmm_xmmm128 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100010a, operands, 0)
            }
            Code::VEX_Vrcpps_ymm_ymmm256 => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100010b, operands, 0)
            }
            Code::VEX_Vrcpss_xmm_xmm_xmmm32 => {
                encoding.validate(false, 2, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100010c, operands, 0)
            }
            Code::VEX_Vaddps_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100010d, operands, 0)
            }
            Code::VEX_Vaddps_ymm_ymm_ymmm256 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100010e, operands, 0)
            }
            Code::VEX_Vaddpd_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100010f, operands, 0)
            }
            Code::VEX_Vaddpd_ymm_ymm_ymmm256 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000110, operands, 0)
            }
            Code::VEX_Vaddss_xmm_xmm_xmmm32 => {
                encoding.validate(false, 3, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000111, operands, 0)
            }
            Code::VEX_Vaddsd_xmm_xmm_xmmm64 => {
                encoding.validate(false, 3, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000112, operands, 0)
            }
            Code::VEX_Vmulps_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000113, operands, 0)
            }
            Code::VEX_Vmulps_ymm_ymm_ymmm256 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000114, operands, 0)
            }
            Code::VEX_Vmulpd_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000115, operands, 0)
            }
            Code::VEX_Vmulpd_ymm_ymm_ymmm256 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000116, operands, 0)
            }
            Code::VEX_Vmulss_xmm_xmm_xmmm32 => {
                encoding.validate(false, 3, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000117, operands, 0)
            }
            Code::VEX_Vmulsd_xmm_xmm_xmmm64 => {
                encoding.validate(false, 3, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000118, operands, 0)
            }
            Code::VEX_Vsubps_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000119, operands, 0)
            }
            Code::VEX_Vsubps_ymm_ymm_ymmm256 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100011a, operands, 0)
            }
            Code::VEX_Vsubpd_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100011b, operands, 0)
            }
            Code::VEX_Vsubpd_ymm_ymm_ymmm256 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100011c, operands, 0)
            }
            Code::VEX_Vsubss_xmm_xmm_xmmm32 => {
                encoding.validate(false, 3, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100011d, operands, 0)
            }
            Code::VEX_Vsubsd_xmm_xmm_xmmm64 => {
                encoding.validate(false, 3, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100011e, operands, 0)
            }
            Code::VEX_Vminps_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100011f, operands, 0)
            }
            Code::VEX_Vminps_ymm_ymm_ymmm256 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000120, operands, 0)
            }
            Code::VEX_Vminpd_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000121, operands, 0)
            }
            Code::VEX_Vminpd_ymm_ymm_ymmm256 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000122, operands, 0)
            }
            Code::VEX_Vminss_xmm_xmm_xmmm32 => {
                encoding.validate(false, 3, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000123, operands, 0)
            }
            Code::VEX_Vminsd_xmm_xmm_xmmm64 => {
                encoding.validate(false, 3, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000124, operands, 0)
            }
            Code::VEX_Vdivps_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000125, operands, 0)
            }
            Code::VEX_Vdivps_ymm_ymm_ymmm256 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000126, operands, 0)
            }
            Code::VEX_Vdivpd_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000127, operands, 0)
            }
            Code::VEX_Vdivpd_ymm_ymm_ymmm256 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000128, operands, 0)
            }
            Code::VEX_Vdivss_xmm_xmm_xmmm32 => {
                encoding.validate(false, 3, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000129, operands, 0)
            }
            Code::VEX_Vdivsd_xmm_xmm_xmmm64 => {
                encoding.validate(false, 3, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100012a, operands, 0)
            }
            Code::VEX_Vmaxps_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100012b, operands, 0)
            }
            Code::VEX_Vmaxps_ymm_ymm_ymmm256 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100012c, operands, 0)
            }
            Code::VEX_Vmaxpd_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100012d, operands, 0)
            }
            Code::VEX_Vmaxpd_ymm_ymm_ymmm256 => {
                encoding.validate(false, 1, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100012e, operands, 0)
            }
            Code::VEX_Vmaxss_xmm_xmm_xmmm32 => {
                encoding.validate(false, 3, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100012f, operands, 0)
            }
            Code::VEX_Vmaxsd_xmm_xmm_xmmm64 => {
                encoding.validate(false, 3, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000130, operands, 0)
            }
            _ => return Ok(None),
        };
        let instruction_value = AvxInstruction::decode_descriptor(descriptor, operands, immediate)
            .map_err(|_| invalid())?;
        bind_address(instruction, instruction_value).map(Some)
    })();
    result.transpose()
}
