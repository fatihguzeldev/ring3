use super::*;
pub(super) fn lower(
    instruction: &Instruction,
    encoding: Encoding,
) -> Option<Result<AvxOperation, DecodeError>> {
    let result: Result<Option<AvxOperation>, DecodeError> = (|| {
        let (descriptor, operands, immediate) = match instruction.code() {
            Code::VEX_Vmovups_xmm_xmmm128 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000301, operands, 0)
            }
            Code::VEX_Vmovups_ymm_ymmm256 => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000302, operands, 0)
            }
            Code::VEX_Vmovupd_xmm_xmmm128 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000301, operands, 0)
            }
            Code::VEX_Vmovupd_ymm_ymmm256 => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000302, operands, 0)
            }
            Code::VEX_Vmovss_xmm_xmm_xmm => {
                encoding.validate(false, 3, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, false)? << 6;
                (0x01000303, operands, 0)
            }
            Code::VEX_Vmovss_xmm_m32 => {
                encoding.validate(false, 3, true)?;
                if instruction.op_kind(1) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 0, false, false)?;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000304, operands, 0)
            }
            Code::VEX_Vmovsd_xmm_xmm_xmm => {
                encoding.validate(false, 2, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, false)? << 6;
                (0x01000305, operands, 0)
            }
            Code::VEX_Vmovsd_xmm_m64 => {
                encoding.validate(false, 2, true)?;
                if instruction.op_kind(1) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 0, false, false)?;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000306, operands, 0)
            }
            Code::VEX_Vmovups_xmmm128_xmm => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, true)?;
                operands |= index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x01000307, operands, 0)
            }
            Code::VEX_Vmovups_ymmm256_ymm => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, true)?;
                operands |= index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x01000308, operands, 0)
            }
            Code::VEX_Vmovupd_xmmm128_xmm => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, true)?;
                operands |= index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x01000307, operands, 0)
            }
            Code::VEX_Vmovupd_ymmm256_ymm => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, true)?;
                operands |= index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x01000308, operands, 0)
            }
            Code::VEX_Vmovss_xmm_xmm_xmm_0F11 => {
                encoding.validate(false, 3, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, false)? << 6;
                (0x01000303, operands, 0)
            }
            Code::VEX_Vmovss_m32_xmm => {
                encoding.validate(false, 3, true)?;
                if instruction.op_kind(0) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x01000309, operands, 0)
            }
            Code::VEX_Vmovsd_xmm_xmm_xmm_0F11 => {
                encoding.validate(false, 2, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, false)? << 6;
                (0x01000305, operands, 0)
            }
            Code::VEX_Vmovsd_m64_xmm => {
                encoding.validate(false, 2, true)?;
                if instruction.op_kind(0) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x0100030a, operands, 0)
            }
            Code::VEX_Vmovhlps_xmm_xmm_xmm => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, false)? << 6;
                (0x0100030b, operands, 0)
            }
            Code::VEX_Vmovlps_xmm_xmm_m64 => {
                encoding.validate(false, 0, false)?;
                if instruction.op_kind(2) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100030c, operands, 0)
            }
            Code::VEX_Vmovlpd_xmm_xmm_m64 => {
                encoding.validate(false, 0, false)?;
                if instruction.op_kind(2) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100030c, operands, 0)
            }
            Code::VEX_Vmovsldup_xmm_xmmm128 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100030d, operands, 0)
            }
            Code::VEX_Vmovsldup_ymm_ymmm256 => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100030e, operands, 0)
            }
            Code::VEX_Vmovddup_xmm_xmmm64 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100030f, operands, 0)
            }
            Code::VEX_Vmovddup_ymm_ymmm256 => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000310, operands, 0)
            }
            Code::VEX_Vmovlps_m64_xmm => {
                encoding.validate(false, 0, true)?;
                if instruction.op_kind(0) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x01000311, operands, 0)
            }
            Code::VEX_Vmovlpd_m64_xmm => {
                encoding.validate(false, 0, true)?;
                if instruction.op_kind(0) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x01000311, operands, 0)
            }
            Code::VEX_Vmovlhps_xmm_xmm_xmm => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, false)? << 6;
                (0x01000312, operands, 0)
            }
            Code::VEX_Vmovhps_xmm_xmm_m64 => {
                encoding.validate(false, 0, false)?;
                if instruction.op_kind(2) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000313, operands, 0)
            }
            Code::VEX_Vmovhpd_xmm_xmm_m64 => {
                encoding.validate(false, 0, false)?;
                if instruction.op_kind(2) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000313, operands, 0)
            }
            Code::VEX_Vmovshdup_xmm_xmmm128 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000314, operands, 0)
            }
            Code::VEX_Vmovshdup_ymm_ymmm256 => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000315, operands, 0)
            }
            Code::VEX_Vmovhps_m64_xmm => {
                encoding.validate(false, 0, true)?;
                if instruction.op_kind(0) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x01000316, operands, 0)
            }
            Code::VEX_Vmovhpd_m64_xmm => {
                encoding.validate(false, 0, true)?;
                if instruction.op_kind(0) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x01000316, operands, 0)
            }
            Code::VEX_Vmovaps_xmm_xmmm128 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000317, operands, 0)
            }
            Code::VEX_Vmovaps_ymm_ymmm256 => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000318, operands, 0)
            }
            Code::VEX_Vmovapd_xmm_xmmm128 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000317, operands, 0)
            }
            Code::VEX_Vmovapd_ymm_ymmm256 => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000318, operands, 0)
            }
            Code::VEX_Vmovaps_xmmm128_xmm => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, true)?;
                operands |= index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x01000319, operands, 0)
            }
            Code::VEX_Vmovaps_ymmm256_ymm => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, true)?;
                operands |= index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x0100031a, operands, 0)
            }
            Code::VEX_Vmovapd_xmmm128_xmm => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, true)?;
                operands |= index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x01000319, operands, 0)
            }
            Code::VEX_Vmovapd_ymmm256_ymm => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, true)?;
                operands |= index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x0100031a, operands, 0)
            }
            Code::VEX_Vmovntps_m128_xmm => {
                encoding.validate(false, 0, true)?;
                if instruction.op_kind(0) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x0100031b, operands, 0)
            }
            Code::VEX_Vmovntps_m256_ymm => {
                encoding.validate(false, 1, true)?;
                if instruction.op_kind(0) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x0100031c, operands, 0)
            }
            Code::VEX_Vmovntpd_m128_xmm => {
                encoding.validate(false, 0, true)?;
                if instruction.op_kind(0) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x0100031b, operands, 0)
            }
            Code::VEX_Vmovntpd_m256_ymm => {
                encoding.validate(false, 1, true)?;
                if instruction.op_kind(0) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x0100031c, operands, 0)
            }
            Code::VEX_Vmovmskps_r32_xmm => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, true, false)?;
                operands |= index(instruction, 1, false, false)? << 6;
                (0x0100031d, operands, 0)
            }
            Code::VEX_Vmovmskps_r32_ymm => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, true, false)?;
                operands |= index(instruction, 1, false, false)? << 6;
                (0x0100031e, operands, 0)
            }
            Code::VEX_Vmovmskpd_r32_xmm => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, true, false)?;
                operands |= index(instruction, 1, false, false)? << 6;
                (0x0100031f, operands, 0)
            }
            Code::VEX_Vmovmskpd_r32_ymm => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, true, false)?;
                operands |= index(instruction, 1, false, false)? << 6;
                (0x01000320, operands, 0)
            }
            Code::VEX_Vmovd_xmm_rm32 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, true, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000321, operands, 0)
            }
            Code::VEX_Vmovdqa_xmm_xmmm128 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000317, operands, 0)
            }
            Code::VEX_Vmovdqa_ymm_ymmm256 | Code::VEX_Vmovntdqa_ymm_m256 => {
                encoding.validate(false, 1, true)?;
                if instruction.code() == Code::VEX_Vmovntdqa_ymm_m256
                    && instruction.op_kind(1) != OpKind::Memory
                {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000318, operands, 0)
            }
            Code::VEX_Vmovdqu_xmm_xmmm128 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000301, operands, 0)
            }
            Code::VEX_Vmovdqu_ymm_ymmm256 => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000302, operands, 0)
            }
            Code::VEX_Vzeroupper => {
                encoding.validate(false, 0, true)?;
                let operands = 0;
                (0x01000322, operands, 0)
            }
            Code::VEX_Vzeroall => {
                encoding.validate(false, 1, true)?;
                let operands = 0;
                (0x01000323, operands, 0)
            }
            Code::VEX_Vmovd_rm32_xmm => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, true, true)?;
                operands |= index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x01000324, operands, 0)
            }
            Code::VEX_Vmovq_xmm_xmmm64 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000325, operands, 0)
            }
            Code::VEX_Vmovdqa_xmmm128_xmm => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, true)?;
                operands |= index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x01000319, operands, 0)
            }
            Code::VEX_Vmovdqa_ymmm256_ymm => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, true)?;
                operands |= index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x0100031a, operands, 0)
            }
            Code::VEX_Vmovdqu_xmmm128_xmm => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, true)?;
                operands |= index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x01000307, operands, 0)
            }
            Code::VEX_Vmovdqu_ymmm256_ymm => {
                encoding.validate(false, 1, true)?;
                let mut operands = index(instruction, 0, false, true)?;
                operands |= index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x01000308, operands, 0)
            }
            Code::VEX_Vldmxcsr_m32 => {
                encoding.validate(false, 0, true)?;
                if instruction.op_kind(0) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = 0;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000326, operands, 0)
            }
            Code::VEX_Vstmxcsr_m32 => {
                encoding.validate(false, 0, true)?;
                if instruction.op_kind(0) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = 0;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x01000327, operands, 0)
            }
            Code::VEX_Vmovq_xmmm64_xmm => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, true)?;
                operands |= index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x01000328, operands, 0)
            }
            Code::VEX_Vmovntdq_m128_xmm => {
                encoding.validate(false, 0, true)?;
                if instruction.op_kind(0) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x0100031b, operands, 0)
            }
            Code::VEX_Vmovntdq_m256_ymm => {
                encoding.validate(false, 1, true)?;
                if instruction.op_kind(0) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 1, false, false)? << 6;
                if instruction.op_kind(0) == OpKind::Memory {
                    operands |= 1 << 13;
                }
                (0x0100031c, operands, 0)
            }
            Code::VEX_Vlddqu_xmm_m128 => {
                encoding.validate(false, 0, true)?;
                if instruction.op_kind(1) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000329, operands, 0)
            }
            Code::VEX_Vlddqu_ymm_m256 => {
                encoding.validate(false, 1, true)?;
                if instruction.op_kind(1) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100032a, operands, 0)
            }
            Code::VEX_Vmovntdqa_xmm_m128 => {
                encoding.validate(false, 0, true)?;
                if instruction.op_kind(1) != OpKind::Memory {
                    return Err(invalid());
                }
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100032b, operands, 0)
            }
            _ => return Ok(None),
        };
        let instruction_value = AvxInstruction::decode_descriptor(descriptor, operands, immediate)
            .map_err(|_| invalid())?;
        let operation = bind_address(instruction, instruction_value)?;
        if instruction.code() == Code::VEX_Vmovntdqa_ymm_m256
            && !crate::cpu::x86::capabilities::AVX2_ENABLED
        {
            return Err(unsupported());
        }
        Ok(Some(operation))
    })();
    result.transpose()
}
