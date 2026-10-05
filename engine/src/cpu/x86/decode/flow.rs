use iced_x86::{Code, Instruction, OpKind};

use super::{
    DecodeError,
    operands::{byte_register, effective_address, location, register, unsupported, value},
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
        Code::Leaved => Operation::Leave,
        Code::Retnd => Operation::Return { stack_adjust: 0 },
        Code::Retnd_imm16 => Operation::Return {
            stack_adjust: instruction.immediate16(),
        },
        Code::Cmovo_r32_rm32
        | Code::Cmovno_r32_rm32
        | Code::Cmovb_r32_rm32
        | Code::Cmovae_r32_rm32
        | Code::Cmove_r32_rm32
        | Code::Cmovne_r32_rm32
        | Code::Cmovbe_r32_rm32
        | Code::Cmova_r32_rm32
        | Code::Cmovs_r32_rm32
        | Code::Cmovns_r32_rm32
        | Code::Cmovp_r32_rm32
        | Code::Cmovnp_r32_rm32
        | Code::Cmovl_r32_rm32
        | Code::Cmovge_r32_rm32
        | Code::Cmovle_r32_rm32
        | Code::Cmovg_r32_rm32 => {
            if instruction.op0_kind() != OpKind::Register
                || instruction.op1_kind() != OpKind::Register
            {
                return Err(unsupported());
            }
            Operation::ConditionalMove {
                condition: condition(instruction.code())?,
                destination: register(instruction.op0_register())?,
                source: register(instruction.op1_register())?,
            }
        }
        Code::Seto_rm8
        | Code::Setno_rm8
        | Code::Setb_rm8
        | Code::Setae_rm8
        | Code::Sete_rm8
        | Code::Setne_rm8
        | Code::Setbe_rm8
        | Code::Seta_rm8
        | Code::Sets_rm8
        | Code::Setns_rm8
        | Code::Setp_rm8
        | Code::Setnp_rm8
        | Code::Setl_rm8
        | Code::Setge_rm8
        | Code::Setle_rm8
        | Code::Setg_rm8 => {
            if instruction.op0_kind() == OpKind::Memory {
                Operation::MemorySetByte {
                    condition: condition(instruction.code())?,
                    address: effective_address(instruction)?,
                }
            } else {
                Operation::SetByte {
                    condition: condition(instruction.code())?,
                    destination: byte_register(instruction.op0_register())?,
                }
            }
        }
        code => Operation::ConditionalJump {
            condition: condition(code)?,
            target: GuestAddress(instruction.near_branch32()),
        },
    })
}

fn condition(code: Code) -> Result<Condition, DecodeError> {
    Ok(match code {
        Code::Jo_rel8_32 | Code::Jo_rel32_32 | Code::Seto_rm8 | Code::Cmovo_r32_rm32 => {
            Condition::Overflow
        }
        Code::Jno_rel8_32 | Code::Jno_rel32_32 | Code::Setno_rm8 | Code::Cmovno_r32_rm32 => {
            Condition::NotOverflow
        }
        Code::Jb_rel8_32 | Code::Jb_rel32_32 | Code::Setb_rm8 | Code::Cmovb_r32_rm32 => {
            Condition::Below
        }
        Code::Jae_rel8_32 | Code::Jae_rel32_32 | Code::Setae_rm8 | Code::Cmovae_r32_rm32 => {
            Condition::AboveOrEqual
        }
        Code::Je_rel8_32 | Code::Je_rel32_32 | Code::Sete_rm8 | Code::Cmove_r32_rm32 => {
            Condition::Equal
        }
        Code::Jne_rel8_32 | Code::Jne_rel32_32 | Code::Setne_rm8 | Code::Cmovne_r32_rm32 => {
            Condition::NotEqual
        }
        Code::Jbe_rel8_32 | Code::Jbe_rel32_32 | Code::Setbe_rm8 | Code::Cmovbe_r32_rm32 => {
            Condition::BelowOrEqual
        }
        Code::Ja_rel8_32 | Code::Ja_rel32_32 | Code::Seta_rm8 | Code::Cmova_r32_rm32 => {
            Condition::Above
        }
        Code::Js_rel8_32 | Code::Js_rel32_32 | Code::Sets_rm8 | Code::Cmovs_r32_rm32 => {
            Condition::Sign
        }
        Code::Jns_rel8_32 | Code::Jns_rel32_32 | Code::Setns_rm8 | Code::Cmovns_r32_rm32 => {
            Condition::NotSign
        }
        Code::Jp_rel8_32 | Code::Jp_rel32_32 | Code::Setp_rm8 | Code::Cmovp_r32_rm32 => {
            Condition::Parity
        }
        Code::Jnp_rel8_32 | Code::Jnp_rel32_32 | Code::Setnp_rm8 | Code::Cmovnp_r32_rm32 => {
            Condition::NotParity
        }
        Code::Jl_rel8_32 | Code::Jl_rel32_32 | Code::Setl_rm8 | Code::Cmovl_r32_rm32 => {
            Condition::Less
        }
        Code::Jge_rel8_32 | Code::Jge_rel32_32 | Code::Setge_rm8 | Code::Cmovge_r32_rm32 => {
            Condition::GreaterOrEqual
        }
        Code::Jle_rel8_32 | Code::Jle_rel32_32 | Code::Setle_rm8 | Code::Cmovle_r32_rm32 => {
            Condition::LessOrEqual
        }
        Code::Jg_rel8_32 | Code::Jg_rel32_32 | Code::Setg_rm8 | Code::Cmovg_r32_rm32 => {
            Condition::Greater
        }
        _ => return Err(unsupported()),
    })
}
