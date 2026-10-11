use iced_x86::{Code, Instruction, OpKind};

use super::{
    DecodeError,
    operands::{
        byte_register, effective_address, location, register, unsupported, value, word_register,
    },
};
use crate::{
    cpu::x86::ir::{BranchTarget, Condition, CountBranchKind, Operation, WordValue},
    memory::GuestAddress,
};

pub(super) fn lower(instruction: &Instruction) -> Result<Operation, DecodeError> {
    if let Some(operation) = super::stack::lower(instruction) {
        return operation;
    }
    Ok(match instruction.code() {
        Code::Jmp_rel8_32 | Code::Jmp_rel32_32 => Operation::Jump {
            target: BranchTarget::Direct(GuestAddress(instruction.near_branch32())),
        },
        Code::Jmp_rel16 => Operation::Jump {
            target: BranchTarget::Direct(GuestAddress(u32::from(instruction.near_branch16()))),
        },
        Code::Loop_rel8_32_ECX
        | Code::Loope_rel8_32_ECX
        | Code::Loopne_rel8_32_ECX
        | Code::Jecxz_rel8_32 => Operation::CountBranch {
            kind: match instruction.code() {
                Code::Loop_rel8_32_ECX => CountBranchKind::Loop,
                Code::Loope_rel8_32_ECX => CountBranchKind::LoopEqual,
                Code::Loopne_rel8_32_ECX => CountBranchKind::LoopNotEqual,
                _ => CountBranchKind::EcxZero,
            },
            target: GuestAddress(instruction.near_branch32()),
        },
        Code::Loop_rel8_32_CX
        | Code::Loope_rel8_32_CX
        | Code::Loopne_rel8_32_CX
        | Code::Jcxz_rel8_32 => Operation::CountBranchWord {
            kind: match instruction.code() {
                Code::Loop_rel8_32_CX => CountBranchKind::Loop,
                Code::Loope_rel8_32_CX => CountBranchKind::LoopEqual,
                Code::Loopne_rel8_32_CX => CountBranchKind::LoopNotEqual,
                _ => CountBranchKind::EcxZero,
            },
            target: GuestAddress(instruction.near_branch32()),
        },
        Code::Jmp_rm32 => Operation::Jump {
            target: BranchTarget::Indirect(location(instruction, 0)?),
        },
        Code::Jmp_rm16 => match instruction.op0_kind() {
            OpKind::Register => Operation::JumpWord {
                source: word_register(instruction.op0_register())?,
            },
            OpKind::Memory => Operation::ReadJumpWord {
                address: effective_address(instruction)?,
            },
            _ => return Err(unsupported()),
        },
        Code::Call_rel32_32 => Operation::Call {
            target: BranchTarget::Direct(GuestAddress(instruction.near_branch32())),
        },
        Code::Call_rel16 => Operation::CallWord {
            target: GuestAddress(u32::from(instruction.near_branch16())),
        },
        Code::Call_rm32 => Operation::Call {
            target: BranchTarget::Indirect(location(instruction, 0)?),
        },
        Code::Call_rm16 => match instruction.op0_kind() {
            OpKind::Register => Operation::CallRegisterWord {
                source: word_register(instruction.op0_register())?,
            },
            OpKind::Memory => Operation::ReadCallWord {
                address: effective_address(instruction)?,
            },
            _ => return Err(unsupported()),
        },
        Code::Push_r32 | Code::Push_rm32 | Code::Pushd_imm32 | Code::Pushd_imm8 => {
            Operation::Push {
                source: value(instruction, 0)?,
            }
        }
        Code::Push_r16 | Code::Push_rm16 | Code::Push_imm16 | Code::Pushw_imm8 => {
            match instruction.op0_kind() {
                OpKind::Register => Operation::PushWord {
                    source: WordValue::Register(word_register(instruction.op0_register())?),
                },
                OpKind::Memory => Operation::PushMemoryWord {
                    source: effective_address(instruction)?,
                },
                OpKind::Immediate16 => Operation::PushWord {
                    source: WordValue::Immediate(instruction.immediate16()),
                },
                OpKind::Immediate8to16 => Operation::PushWord {
                    source: WordValue::Immediate(instruction.immediate8to16() as u16),
                },
                _ => return Err(unsupported()),
            }
        }
        Code::Pushfd => Operation::PushFlags,
        Code::Pushfw => Operation::PushFlagsWord,
        Code::Pop_r32 | Code::Pop_rm32 => Operation::Pop {
            destination: location(instruction, 0)?,
        },
        Code::Pop_r16 | Code::Pop_rm16 => match instruction.op0_kind() {
            OpKind::Register => Operation::PopWord {
                destination: word_register(instruction.op0_register())?,
            },
            OpKind::Memory => Operation::PopMemoryWord {
                destination: effective_address(instruction)?,
            },
            _ => return Err(unsupported()),
        },
        Code::Popad => Operation::PopAll,
        Code::Popaw => Operation::PopAllWord,
        Code::Enterd_imm16_imm8 => Operation::EnterZero,
        Code::Enterw_imm16_imm8 => Operation::EnterZeroWord,
        Code::Leaved => Operation::Leave,
        Code::Leavew => Operation::LeaveWord,
        Code::Retnd => Operation::Return { stack_adjust: 0 },
        Code::Retnd_imm16 => Operation::Return {
            stack_adjust: instruction.immediate16(),
        },
        Code::Retnw => Operation::ReturnWord { stack_adjust: 0 },
        Code::Retnw_imm16 => Operation::ReturnWord {
            stack_adjust: instruction.immediate16(),
        },
        Code::Cmovo_r16_rm16
        | Code::Cmovno_r16_rm16
        | Code::Cmovb_r16_rm16
        | Code::Cmovae_r16_rm16
        | Code::Cmove_r16_rm16
        | Code::Cmovne_r16_rm16
        | Code::Cmovbe_r16_rm16
        | Code::Cmova_r16_rm16
        | Code::Cmovs_r16_rm16
        | Code::Cmovns_r16_rm16
        | Code::Cmovp_r16_rm16
        | Code::Cmovnp_r16_rm16
        | Code::Cmovl_r16_rm16
        | Code::Cmovge_r16_rm16
        | Code::Cmovle_r16_rm16
        | Code::Cmovg_r16_rm16 => {
            let condition = condition(instruction.code())?;
            let destination = word_register(instruction.op0_register())?;
            match instruction.op1_kind() {
                OpKind::Memory => Operation::ReadConditionalMoveWord {
                    condition,
                    destination,
                    address: effective_address(instruction)?,
                },
                OpKind::Register => Operation::ConditionalMoveWord {
                    condition,
                    destination,
                    source: word_register(instruction.op1_register())?,
                },
                _ => return Err(unsupported()),
            }
        }
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
            let condition = condition(instruction.code())?;
            let destination = register(instruction.op0_register())?;
            match instruction.op1_kind() {
                OpKind::Memory => Operation::ReadConditionalMove {
                    condition,
                    destination,
                    address: effective_address(instruction)?,
                },
                OpKind::Register => Operation::ConditionalMove {
                    condition,
                    destination,
                    source: register(instruction.op1_register())?,
                },
                _ => return Err(unsupported()),
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
            target: GuestAddress(if instruction.op0_kind() == OpKind::NearBranch16 {
                u32::from(instruction.near_branch16())
            } else {
                instruction.near_branch32()
            }),
        },
    })
}

fn condition(code: Code) -> Result<Condition, DecodeError> {
    Ok(match code {
        Code::Jo_rel8_32
        | Code::Jo_rel16
        | Code::Jo_rel32_32
        | Code::Seto_rm8
        | Code::Cmovo_r16_rm16
        | Code::Cmovo_r32_rm32 => Condition::Overflow,
        Code::Jno_rel8_32
        | Code::Jno_rel16
        | Code::Jno_rel32_32
        | Code::Setno_rm8
        | Code::Cmovno_r16_rm16
        | Code::Cmovno_r32_rm32 => Condition::NotOverflow,
        Code::Jb_rel8_32
        | Code::Jb_rel16
        | Code::Jb_rel32_32
        | Code::Setb_rm8
        | Code::Cmovb_r16_rm16
        | Code::Cmovb_r32_rm32 => Condition::Below,
        Code::Jae_rel8_32
        | Code::Jae_rel16
        | Code::Jae_rel32_32
        | Code::Setae_rm8
        | Code::Cmovae_r16_rm16
        | Code::Cmovae_r32_rm32 => Condition::AboveOrEqual,
        Code::Je_rel8_32
        | Code::Je_rel16
        | Code::Je_rel32_32
        | Code::Sete_rm8
        | Code::Cmove_r16_rm16
        | Code::Cmove_r32_rm32 => Condition::Equal,
        Code::Jne_rel8_32
        | Code::Jne_rel16
        | Code::Jne_rel32_32
        | Code::Setne_rm8
        | Code::Cmovne_r16_rm16
        | Code::Cmovne_r32_rm32 => Condition::NotEqual,
        Code::Jbe_rel8_32
        | Code::Jbe_rel16
        | Code::Jbe_rel32_32
        | Code::Setbe_rm8
        | Code::Cmovbe_r16_rm16
        | Code::Cmovbe_r32_rm32 => Condition::BelowOrEqual,
        Code::Ja_rel8_32
        | Code::Ja_rel16
        | Code::Ja_rel32_32
        | Code::Seta_rm8
        | Code::Cmova_r16_rm16
        | Code::Cmova_r32_rm32 => Condition::Above,
        Code::Js_rel8_32
        | Code::Js_rel16
        | Code::Js_rel32_32
        | Code::Sets_rm8
        | Code::Cmovs_r16_rm16
        | Code::Cmovs_r32_rm32 => Condition::Sign,
        Code::Jns_rel8_32
        | Code::Jns_rel16
        | Code::Jns_rel32_32
        | Code::Setns_rm8
        | Code::Cmovns_r16_rm16
        | Code::Cmovns_r32_rm32 => Condition::NotSign,
        Code::Jp_rel8_32
        | Code::Jp_rel16
        | Code::Jp_rel32_32
        | Code::Setp_rm8
        | Code::Cmovp_r16_rm16
        | Code::Cmovp_r32_rm32 => Condition::Parity,
        Code::Jnp_rel8_32
        | Code::Jnp_rel16
        | Code::Jnp_rel32_32
        | Code::Setnp_rm8
        | Code::Cmovnp_r16_rm16
        | Code::Cmovnp_r32_rm32 => Condition::NotParity,
        Code::Jl_rel8_32
        | Code::Jl_rel16
        | Code::Jl_rel32_32
        | Code::Setl_rm8
        | Code::Cmovl_r16_rm16
        | Code::Cmovl_r32_rm32 => Condition::Less,
        Code::Jge_rel8_32
        | Code::Jge_rel16
        | Code::Jge_rel32_32
        | Code::Setge_rm8
        | Code::Cmovge_r16_rm16
        | Code::Cmovge_r32_rm32 => Condition::GreaterOrEqual,
        Code::Jle_rel8_32
        | Code::Jle_rel16
        | Code::Jle_rel32_32
        | Code::Setle_rm8
        | Code::Cmovle_r16_rm16
        | Code::Cmovle_r32_rm32 => Condition::LessOrEqual,
        Code::Jg_rel8_32
        | Code::Jg_rel16
        | Code::Jg_rel32_32
        | Code::Setg_rm8
        | Code::Cmovg_r16_rm16
        | Code::Cmovg_r32_rm32 => Condition::Greater,
        _ => return Err(unsupported()),
    })
}
