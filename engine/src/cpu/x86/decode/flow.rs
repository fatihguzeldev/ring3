use iced_x86::{Code, Instruction};

use super::{
    DecodeError,
    operands::{location, unsupported, value},
};
use crate::{
    cpu::x86::ir::{BranchTarget, Condition, Operation},
    memory::GuestAddress,
};

pub(super) fn lower(instruction: &Instruction) -> Result<Operation, DecodeError> {
    Ok(match instruction.code() {
        Code::Jmp_rel8_32 | Code::Jmp_rel32_32 => Operation::Jump {
            target: BranchTarget::Direct(GuestAddress(instruction.near_branch32())),
        },
        Code::Jmp_rm32 => Operation::Jump {
            target: BranchTarget::Indirect(location(instruction, 0)?),
        },
        Code::Call_rel32_32 => Operation::Call {
            target: BranchTarget::Direct(GuestAddress(instruction.near_branch32())),
        },
        Code::Call_rm32 => Operation::Call {
            target: BranchTarget::Indirect(location(instruction, 0)?),
        },
        Code::Push_r32 | Code::Push_rm32 | Code::Pushd_imm32 | Code::Pushd_imm8 => {
            Operation::Push {
                source: value(instruction, 0)?,
            }
        }
        Code::Pop_r32 | Code::Pop_rm32 => Operation::Pop {
            destination: location(instruction, 0)?,
        },
        Code::Retnd => Operation::Return { stack_adjust: 0 },
        Code::Retnd_imm16 => Operation::Return {
            stack_adjust: instruction.immediate16(),
        },
        code => Operation::ConditionalJump {
            condition: condition(code)?,
            target: GuestAddress(instruction.near_branch32()),
        },
    })
}

fn condition(code: Code) -> Result<Condition, DecodeError> {
    Ok(match code {
        Code::Jo_rel8_32 | Code::Jo_rel32_32 => Condition::Overflow,
        Code::Jno_rel8_32 | Code::Jno_rel32_32 => Condition::NotOverflow,
        Code::Jb_rel8_32 | Code::Jb_rel32_32 => Condition::Below,
        Code::Jae_rel8_32 | Code::Jae_rel32_32 => Condition::AboveOrEqual,
        Code::Je_rel8_32 | Code::Je_rel32_32 => Condition::Equal,
        Code::Jne_rel8_32 | Code::Jne_rel32_32 => Condition::NotEqual,
        Code::Jbe_rel8_32 | Code::Jbe_rel32_32 => Condition::BelowOrEqual,
        Code::Ja_rel8_32 | Code::Ja_rel32_32 => Condition::Above,
        Code::Js_rel8_32 | Code::Js_rel32_32 => Condition::Sign,
        Code::Jns_rel8_32 | Code::Jns_rel32_32 => Condition::NotSign,
        Code::Jp_rel8_32 | Code::Jp_rel32_32 => Condition::Parity,
        Code::Jnp_rel8_32 | Code::Jnp_rel32_32 => Condition::NotParity,
        Code::Jl_rel8_32 | Code::Jl_rel32_32 => Condition::Less,
        Code::Jge_rel8_32 | Code::Jge_rel32_32 => Condition::GreaterOrEqual,
        Code::Jle_rel8_32 | Code::Jle_rel32_32 => Condition::LessOrEqual,
        Code::Jg_rel8_32 | Code::Jg_rel32_32 => Condition::Greater,
        _ => return Err(unsupported()),
    })
}
