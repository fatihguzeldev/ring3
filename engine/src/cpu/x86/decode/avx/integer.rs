use super::*;
pub(super) fn lower(
    instruction: &Instruction,
    encoding: Encoding,
) -> Option<Result<AvxOperation, DecodeError>> {
    let result: Result<Option<AvxOperation>, DecodeError> = (|| {
        let (descriptor, operands, immediate) = match instruction.code() {
            Code::VEX_Vpcmpgtb_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000501, operands, 0)
            }
            Code::VEX_Vpcmpgtw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000502, operands, 0)
            }
            Code::VEX_Vpcmpgtd_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000503, operands, 0)
            }
            Code::VEX_Vpcmpeqb_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000504, operands, 0)
            }
            Code::VEX_Vpcmpeqw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000505, operands, 0)
            }
            Code::VEX_Vpcmpeqd_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000506, operands, 0)
            }
            Code::VEX_Vpaddq_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000507, operands, 0)
            }
            Code::VEX_Vpsubusb_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000508, operands, 0)
            }
            Code::VEX_Vpsubusw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000509, operands, 0)
            }
            Code::VEX_Vpminub_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100050a, operands, 0)
            }
            Code::VEX_Vpaddusb_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100050b, operands, 0)
            }
            Code::VEX_Vpaddusw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100050c, operands, 0)
            }
            Code::VEX_Vpmaxub_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100050d, operands, 0)
            }
            Code::VEX_Vpavgb_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100050e, operands, 0)
            }
            Code::VEX_Vpavgw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100050f, operands, 0)
            }
            Code::VEX_Vpsubsb_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000510, operands, 0)
            }
            Code::VEX_Vpsubsw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000511, operands, 0)
            }
            Code::VEX_Vpminsw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000512, operands, 0)
            }
            Code::VEX_Vpaddsb_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000513, operands, 0)
            }
            Code::VEX_Vpaddsw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000514, operands, 0)
            }
            Code::VEX_Vpmaxsw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000515, operands, 0)
            }
            Code::VEX_Vpsubb_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000516, operands, 0)
            }
            Code::VEX_Vpsubw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000517, operands, 0)
            }
            Code::VEX_Vpsubd_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000518, operands, 0)
            }
            Code::VEX_Vpsubq_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000519, operands, 0)
            }
            Code::VEX_Vpaddb_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100051a, operands, 0)
            }
            Code::VEX_Vpaddw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100051b, operands, 0)
            }
            Code::VEX_Vpaddd_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100051c, operands, 0)
            }
            Code::VEX_Vpsignb_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100051d, operands, 0)
            }
            Code::VEX_Vpsignw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100051e, operands, 0)
            }
            Code::VEX_Vpsignd_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100051f, operands, 0)
            }
            Code::VEX_Vpabsb_xmm_xmmm128 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000520, operands, 0)
            }
            Code::VEX_Vpabsw_xmm_xmmm128 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000521, operands, 0)
            }
            Code::VEX_Vpabsd_xmm_xmmm128 => {
                encoding.validate(false, 0, true)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, true)? << 6;
                if instruction.op_kind(1) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000522, operands, 0)
            }
            Code::VEX_Vpcmpeqq_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000523, operands, 0)
            }
            Code::VEX_Vpcmpgtq_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000524, operands, 0)
            }
            Code::VEX_Vpminsb_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000525, operands, 0)
            }
            Code::VEX_Vpminsd_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000526, operands, 0)
            }
            Code::VEX_Vpminuw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000527, operands, 0)
            }
            Code::VEX_Vpminud_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000528, operands, 0)
            }
            Code::VEX_Vpmaxsb_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x01000529, operands, 0)
            }
            Code::VEX_Vpmaxsd_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100052a, operands, 0)
            }
            Code::VEX_Vpmaxuw_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100052b, operands, 0)
            }
            Code::VEX_Vpmaxud_xmm_xmm_xmmm128 => {
                encoding.validate(false, 0, false)?;
                let mut operands = index(instruction, 0, false, false)?;
                operands |= index(instruction, 1, false, false)? << 3;
                operands |= index(instruction, 2, false, true)? << 6;
                if instruction.op_kind(2) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (0x0100052c, operands, 0)
            }
            _ => {
                let descriptor = match instruction.code() {
                    Code::VEX_Vpcmpgtb_ymm_ymm_ymmm256 => 0x01000581,
                    Code::VEX_Vpcmpgtw_ymm_ymm_ymmm256 => 0x01000582,
                    Code::VEX_Vpcmpgtd_ymm_ymm_ymmm256 => 0x01000583,
                    Code::VEX_Vpcmpeqb_ymm_ymm_ymmm256 => 0x01000584,
                    Code::VEX_Vpcmpeqw_ymm_ymm_ymmm256 => 0x01000585,
                    Code::VEX_Vpcmpeqd_ymm_ymm_ymmm256 => 0x01000586,
                    Code::VEX_Vpcmpeqq_ymm_ymm_ymmm256 => 0x010005a3,
                    Code::VEX_Vpcmpgtq_ymm_ymm_ymmm256 => 0x010005a4,
                    Code::VEX_Vpaddq_ymm_ymm_ymmm256 => 0x01000587,
                    Code::VEX_Vpsubb_ymm_ymm_ymmm256 => 0x01000596,
                    Code::VEX_Vpsubw_ymm_ymm_ymmm256 => 0x01000597,
                    Code::VEX_Vpsubd_ymm_ymm_ymmm256 => 0x01000598,
                    Code::VEX_Vpsubq_ymm_ymm_ymmm256 => 0x01000599,
                    Code::VEX_Vpaddb_ymm_ymm_ymmm256 => 0x0100059a,
                    Code::VEX_Vpaddw_ymm_ymm_ymmm256 => 0x0100059b,
                    Code::VEX_Vpaddd_ymm_ymm_ymmm256 => 0x0100059c,
                    Code::VEX_Vpsubusb_ymm_ymm_ymmm256 => 0x01000588,
                    Code::VEX_Vpsubusw_ymm_ymm_ymmm256 => 0x01000589,
                    Code::VEX_Vpminub_ymm_ymm_ymmm256 => 0x0100058a,
                    Code::VEX_Vpaddusb_ymm_ymm_ymmm256 => 0x0100058b,
                    Code::VEX_Vpaddusw_ymm_ymm_ymmm256 => 0x0100058c,
                    Code::VEX_Vpmaxub_ymm_ymm_ymmm256 => 0x0100058d,
                    Code::VEX_Vpavgb_ymm_ymm_ymmm256 => 0x0100058e,
                    Code::VEX_Vpavgw_ymm_ymm_ymmm256 => 0x0100058f,
                    Code::VEX_Vpsubsb_ymm_ymm_ymmm256 => 0x01000590,
                    Code::VEX_Vpsubsw_ymm_ymm_ymmm256 => 0x01000591,
                    Code::VEX_Vpminsw_ymm_ymm_ymmm256 => 0x01000592,
                    Code::VEX_Vpaddsb_ymm_ymm_ymmm256 => 0x01000593,
                    Code::VEX_Vpaddsw_ymm_ymm_ymmm256 => 0x01000594,
                    Code::VEX_Vpmaxsw_ymm_ymm_ymmm256 => 0x01000595,
                    Code::VEX_Vpsignb_ymm_ymm_ymmm256 => 0x0100059d,
                    Code::VEX_Vpsignw_ymm_ymm_ymmm256 => 0x0100059e,
                    Code::VEX_Vpsignd_ymm_ymm_ymmm256 => 0x0100059f,
                    Code::VEX_Vpabsb_ymm_ymmm256 => 0x010005a0,
                    Code::VEX_Vpabsw_ymm_ymmm256 => 0x010005a1,
                    Code::VEX_Vpabsd_ymm_ymmm256 => 0x010005a2,
                    Code::VEX_Vpminsb_ymm_ymm_ymmm256 => 0x010005a5,
                    Code::VEX_Vpminsd_ymm_ymm_ymmm256 => 0x010005a6,
                    Code::VEX_Vpminuw_ymm_ymm_ymmm256 => 0x010005a7,
                    Code::VEX_Vpminud_ymm_ymm_ymmm256 => 0x010005a8,
                    Code::VEX_Vpmaxsb_ymm_ymm_ymmm256 => 0x010005a9,
                    Code::VEX_Vpmaxsd_ymm_ymm_ymmm256 => 0x010005aa,
                    Code::VEX_Vpmaxuw_ymm_ymm_ymmm256 => 0x010005ab,
                    Code::VEX_Vpmaxud_ymm_ymm_ymmm256 => 0x010005ac,
                    _ => return Ok(None),
                };
                let unary = matches!(
                    instruction.code(),
                    Code::VEX_Vpabsb_ymm_ymmm256
                        | Code::VEX_Vpabsw_ymm_ymmm256
                        | Code::VEX_Vpabsd_ymm_ymmm256
                );
                encoding.validate(false, 1, unary)?;
                let mut operands = index(instruction, 0, false, false)?;
                if !unary {
                    operands |= index(instruction, 1, false, false)? << 3;
                }
                let source = if unary { 1 } else { 2 };
                operands |= index(instruction, source, false, true)? << 6;
                if instruction.op_kind(source) == OpKind::Memory {
                    operands |= 1 << 12;
                }
                (descriptor, operands, 0)
            }
        };
        let instruction_value = AvxInstruction::decode_descriptor(descriptor, operands, immediate)
            .map_err(|_| invalid())?;
        let operation = bind_address(instruction, instruction_value)?;
        if !crate::cpu::x86::capabilities::AVX2_ENABLED && descriptor & 0x80 != 0 {
            return Err(unsupported());
        }
        Ok(Some(operation))
    })();
    result.transpose()
}
