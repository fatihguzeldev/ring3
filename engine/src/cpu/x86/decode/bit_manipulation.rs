use iced_x86::{Code, Instruction, OpKind};

use super::{
    DecodeError,
    operands::{effective_address, register, unsupported},
};
use crate::cpu::x86::{
    bits::BitManipulationKind,
    ir::{Operation, Value32},
};

pub(super) fn lower(instruction: &Instruction) -> Option<Result<Operation, DecodeError>> {
    let source_index = match instruction.code() {
        Code::VEX_Andn_r32_r32_rm32 => 2,
        Code::VEX_Bextr_r32_rm32_r32
        | Code::VEX_Blsi_r32_rm32
        | Code::VEX_Blsmsk_r32_rm32
        | Code::VEX_Blsr_r32_rm32 => 1,
        _ => return None,
    };
    Some((|| {
        let destination = register(instruction.op0_register())?;
        let kind = match instruction.code() {
            Code::VEX_Andn_r32_r32_rm32 => BitManipulationKind::AndNot {
                left: register(instruction.op1_register())?,
            },
            Code::VEX_Bextr_r32_rm32_r32 => BitManipulationKind::Extract {
                control: register(instruction.op2_register())?,
            },
            Code::VEX_Blsi_r32_rm32 => BitManipulationKind::IsolateLowest,
            Code::VEX_Blsmsk_r32_rm32 => BitManipulationKind::MaskLowest,
            Code::VEX_Blsr_r32_rm32 => BitManipulationKind::ResetLowest,
            _ => unreachable!(),
        };
        let source = match instruction.op_kind(source_index) {
            OpKind::Register => Value32::Register(register(instruction.op_register(source_index))?),
            OpKind::Memory => Value32::Memory(effective_address(instruction)?),
            _ => return Err(unsupported()),
        };
        Ok(Operation::BitManipulation {
            kind,
            destination,
            source,
        })
    })())
}
