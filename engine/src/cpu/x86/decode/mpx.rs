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
    !CPU_ID_LEAVES.iter().any(|leaf| {
        leaf.function == 7 && leaf.subfunction == Some(0) && leaf.registers[1] & (1 << 14) != 0
    })
}

#[cfg(test)]
mod tests;
