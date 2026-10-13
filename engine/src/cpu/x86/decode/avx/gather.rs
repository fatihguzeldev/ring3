use super::*;
use crate::cpu::x86::avx::{GatherAddress, GatherGeometry, VectorWidth};

pub(super) fn lower(
    instruction: &Instruction,
    encoding: Encoding,
) -> Option<Result<AvxOperation, DecodeError>> {
    let id = match instruction.code() {
        Code::VEX_Vpgatherdd_xmm_vm32x_xmm | Code::VEX_Vgatherdps_xmm_vm32x_xmm => 1,
        Code::VEX_Vpgatherdd_ymm_vm32y_ymm | Code::VEX_Vgatherdps_ymm_vm32y_ymm => 2,
        Code::VEX_Vpgatherdq_xmm_vm32x_xmm | Code::VEX_Vgatherdpd_xmm_vm32x_xmm => 3,
        Code::VEX_Vpgatherdq_ymm_vm32x_ymm | Code::VEX_Vgatherdpd_ymm_vm32x_ymm => 4,
        Code::VEX_Vpgatherqd_xmm_vm64x_xmm | Code::VEX_Vgatherqps_xmm_vm64x_xmm => 5,
        Code::VEX_Vpgatherqd_xmm_vm64y_xmm | Code::VEX_Vgatherqps_xmm_vm64y_xmm => 6,
        Code::VEX_Vpgatherqq_xmm_vm64x_xmm | Code::VEX_Vgatherqpd_xmm_vm64x_xmm => 7,
        Code::VEX_Vpgatherqq_ymm_vm64y_ymm | Code::VEX_Vgatherqpd_ymm_vm64y_ymm => 8,
        _ => return None,
    };
    Some((|| {
        let geometry = GatherGeometry::from_id(id).ok_or_else(invalid)?;
        encoding.validate(
            false,
            u8::from(geometry.vector_length() == VectorWidth::Ymm),
            false,
        )?;
        encoding.validate_w(geometry.data_bytes() == 8)?;
        if instruction.op_count() != 3
            || instruction.op0_kind() != OpKind::Register
            || instruction.op2_kind() != OpKind::Register
            || instruction.op1_kind() != OpKind::Memory
            || instruction.memory_base().is_gpr16()
            || instruction.memory_displ_size() == 2
        {
            return Err(invalid());
        }
        let destination = vector(instruction.op0_register(), geometry.active_bytes())?;
        let mask = vector(instruction.op2_register(), geometry.active_bytes())?;
        let index = vector(instruction.memory_index(), geometry.index_register_bytes())?;
        let scale = match instruction.memory_index_scale() {
            1 => 0,
            2 => 1,
            4 => 2,
            8 => 3,
            _ => return Err(invalid()),
        };
        let operands = destination | (index << 3) | (mask << 9) | (1 << 12);
        let value = AvxInstruction::decode_descriptor(
            0x01001400 | u32::from(geometry.id()),
            operands,
            scale,
        )
        .map_err(|_| invalid())?;
        let base = match instruction.memory_base() {
            Register::None => None,
            base => Some(register(base)?),
        };
        let segment = match instruction.segment_prefix() {
            Register::ES => Some(SegmentRegister::Es),
            Register::CS => Some(SegmentRegister::Cs),
            Register::SS => Some(SegmentRegister::Ss),
            Register::DS => Some(SegmentRegister::Ds),
            Register::FS => Some(SegmentRegister::Fs),
            Register::GS => Some(SegmentRegister::Gs),
            _ => None,
        };
        let address = GatherAddress::new(base, instruction.memory_displacement32(), segment);
        let operation = AvxOperation::new_gather(value, address).map_err(|_| invalid())?;
        if !crate::cpu::x86::capabilities::AVX2_ENABLED {
            return Err(unsupported());
        }
        Ok(operation)
    })())
}

fn vector(register: Register, bytes: usize) -> Result<u32, DecodeError> {
    let first = if bytes > 16 {
        Register::YMM0
    } else {
        Register::XMM0
    };
    let value = (register as u32).wrapping_sub(first as u32);
    if value >= 8 {
        return Err(invalid());
    }
    Ok(value)
}

#[cfg(test)]
#[path = "gather/native.rs"]
mod tests;
