use iced_x86::{Code, Instruction, OpKind};

use super::{
    DecodeError,
    operands::{effective_address, register, unsupported},
};
use crate::cpu::x86::{
    bits::BitManipulationKind,
    ir::{Operation, Value32},
};

pub(super) fn recognizes(code: Code) -> bool {
    matches!(
        code,
        Code::VEX_Mulx_r32_r32_rm32
            | Code::VEX_Andn_r32_r32_rm32
            | Code::VEX_Pdep_r32_r32_rm32
            | Code::VEX_Pext_r32_r32_rm32
            | Code::VEX_Bextr_r32_rm32_r32
            | Code::VEX_Blsi_r32_rm32
            | Code::VEX_Blsmsk_r32_rm32
            | Code::VEX_Blsr_r32_rm32
            | Code::VEX_Bzhi_r32_rm32_r32
            | Code::VEX_Shlx_r32_rm32_r32
            | Code::VEX_Shrx_r32_rm32_r32
            | Code::VEX_Sarx_r32_rm32_r32
            | Code::VEX_Rorx_r32_rm32_imm8
    )
}

pub(super) fn lower(instruction: &Instruction) -> Option<Result<Operation, DecodeError>> {
    if instruction.code() == Code::VEX_Mulx_r32_r32_rm32 {
        return Some((|| {
            Ok(Operation::MultiplyExtended {
                high: register(instruction.op0_register())?,
                low: register(instruction.op1_register())?,
                source: source(instruction, 2)?,
            })
        })());
    }
    let source_index = match instruction.code() {
        Code::VEX_Andn_r32_r32_rm32 | Code::VEX_Pdep_r32_r32_rm32 | Code::VEX_Pext_r32_r32_rm32 => {
            2
        }
        Code::VEX_Bextr_r32_rm32_r32
        | Code::VEX_Blsi_r32_rm32
        | Code::VEX_Blsmsk_r32_rm32
        | Code::VEX_Blsr_r32_rm32
        | Code::VEX_Bzhi_r32_rm32_r32
        | Code::VEX_Shlx_r32_rm32_r32
        | Code::VEX_Shrx_r32_rm32_r32
        | Code::VEX_Sarx_r32_rm32_r32
        | Code::VEX_Rorx_r32_rm32_imm8 => 1,
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
            Code::VEX_Bzhi_r32_rm32_r32 => BitManipulationKind::ZeroHigh {
                index: register(instruction.op2_register())?,
            },
            Code::VEX_Pdep_r32_r32_rm32 => BitManipulationKind::Deposit {
                value: register(instruction.op1_register())?,
            },
            Code::VEX_Pext_r32_r32_rm32 => BitManipulationKind::ExtractMasked {
                value: register(instruction.op1_register())?,
            },
            Code::VEX_Shlx_r32_rm32_r32 => BitManipulationKind::ShiftLeft {
                count: register(instruction.op2_register())?,
            },
            Code::VEX_Shrx_r32_rm32_r32 => BitManipulationKind::ShiftLogicalRight {
                count: register(instruction.op2_register())?,
            },
            Code::VEX_Sarx_r32_rm32_r32 => BitManipulationKind::ShiftArithmeticRight {
                count: register(instruction.op2_register())?,
            },
            Code::VEX_Rorx_r32_rm32_imm8 => BitManipulationKind::RotateRight {
                count: instruction.immediate8(),
            },
            _ => unreachable!(),
        };
        Ok(Operation::BitManipulation {
            kind,
            destination,
            source: source(instruction, source_index)?,
        })
    })())
}

fn source(instruction: &Instruction, index: u32) -> Result<Value32, DecodeError> {
    match instruction.op_kind(index) {
        OpKind::Register => register(instruction.op_register(index)).map(Value32::Register),
        OpKind::Memory => effective_address(instruction).map(Value32::Memory),
        _ => Err(unsupported()),
    }
}
