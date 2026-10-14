use crate::cpu::x86::capabilities::CPU_ID_LEAVES;
use iced_x86::{Code, CodeSize, Instruction};

// absent mpx makes only this opcode family a nop, including memory and address16 forms.
pub(super) fn disabled_nop(instruction: &Instruction) -> bool {
    if !matches!(instruction.code_size(), CodeSize::Code16 | CodeSize::Code32)
        || !matches!(
            instruction.code(),
            Code::Reservednop_rm16_r16_0F1A
                | Code::Reservednop_rm32_r32_0F1A
                | Code::Reservednop_rm16_r16_0F1B
                | Code::Reservednop_rm32_r32_0F1B
        )
    {
        return false;
    }
    mpx_absent()
}

// the disabled bounds prefix retains ordinary near-transfer semantics.
pub(super) fn disabled_branch_prefix(instruction: &Instruction) -> bool {
    instruction.has_repne_prefix()
        && matches!(instruction.code_size(), CodeSize::Code16 | CodeSize::Code32)
        && crate::cpu::x86::capabilities::XCR0 & 0x18 == 0
        && mpx_absent()
        && (instruction.is_jcc_short_or_near()
            || matches!(
                instruction.code(),
                Code::Call_rel16
                    | Code::Call_rel32_32
                    | Code::Call_rm16
                    | Code::Call_rm32
                    | Code::Retnw
                    | Code::Retnd
                    | Code::Retnw_imm16
                    | Code::Retnd_imm16
                    | Code::Jmp_rel16
                    | Code::Jmp_rel32_32
                    | Code::Jmp_rm16
                    | Code::Jmp_rm32
            ))
}

fn mpx_absent() -> bool {
    !CPU_ID_LEAVES.iter().any(|leaf| {
        leaf.function == 7 && leaf.subfunction == Some(0) && leaf.registers[1] & (1 << 14) != 0
    })
}

#[cfg(test)]
mod tests;
