use super::{
    DecodeError,
    operands::{effective_address, register, word_register},
};
use crate::cpu::x86::{
    SystemRegisterQuery, UserSystemRegisterBank32,
    ir::{Location32, Operation, Value32, WordValue},
};
use iced_x86::{Code, Instruction, OpKind};

pub(super) fn is_query(code: Code) -> bool {
    matches!(
        code,
        Code::Sgdt_m1632_16
            | Code::Sgdt_m1632
            | Code::Sidt_m1632_16
            | Code::Sidt_m1632
            | Code::Sldt_rm16
            | Code::Sldt_r32m16
            | Code::Str_rm16
            | Code::Str_r32m16
            | Code::Smsw_rm16
            | Code::Smsw_r32m16
    )
}

pub(super) fn lower(
    instruction: &Instruction,
    bank: Option<UserSystemRegisterBank32>,
) -> Option<Result<Operation, DecodeError>> {
    if !is_query(instruction.code()) {
        return None;
    }
    Some((|| {
        let query = match instruction.code() {
            Code::Sgdt_m1632_16 | Code::Sgdt_m1632 => SystemRegisterQuery::Gdtr,
            Code::Sidt_m1632_16 | Code::Sidt_m1632 => SystemRegisterQuery::Idtr,
            Code::Sldt_rm16 | Code::Sldt_r32m16 => SystemRegisterQuery::Ldtr,
            Code::Str_rm16 | Code::Str_r32m16 => SystemRegisterQuery::Tr,
            _ => SystemRegisterQuery::MachineStatusWord,
        };
        let missing = DecodeError::MissingSystemRegisterAuthority(query);
        let bank = bank.ok_or(missing)?;
        if matches!(query, SystemRegisterQuery::Gdtr | SystemRegisterQuery::Idtr) {
            let value = if query == SystemRegisterQuery::Gdtr {
                bank.gdtr
            } else {
                bank.idtr
            }
            .ok_or(missing)?;
            return Ok(Operation::StoreSystemDescriptor {
                address: effective_address(instruction)?,
                limit: value.limit,
                base: value.base,
            });
        }
        let value = bank.value(query).ok_or(missing)?;
        if instruction.op0_kind() == OpKind::Memory {
            return Ok(Operation::StoreWord {
                address: effective_address(instruction)?,
                source: WordValue::Immediate(value),
            });
        }
        // SLDT follows the declared P6 zero-extension policy; STR requires it.
        // Legacy SMSW high16 is undefined: Ring3 deliberately preserves that old destination portion.
        if matches!(instruction.code(), Code::Sldt_r32m16 | Code::Str_r32m16) {
            Ok(Operation::Move {
                destination: Location32::Register(register(instruction.op0_register())?),
                source: Value32::Immediate(u32::from(value)),
            })
        } else {
            Ok(Operation::MoveWord {
                destination: word_register(instruction.op0_register())
                    .or_else(|_| register(instruction.op0_register()))?,
                source: WordValue::Immediate(value),
            })
        }
    })())
}

#[cfg(test)]
mod tests;
