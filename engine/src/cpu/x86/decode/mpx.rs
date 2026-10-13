use crate::cpu::x86::capabilities::CPU_ID_LEAVES;
use iced_x86::{Code, CodeSize, Instruction, OpKind};

/// iced DecoderOptions::NONE decodes canonical absent-MPX BNDMOV as these reserved NOP codes.
/// only canonical register encodings are currently admitted.
pub(super) fn canonical_nop(instruction: &Instruction, bytes: &[u8]) -> bool {
    let canonical = matches!(
        bytes,
        [
            0x66,
            0x0f,
            0x1a | 0x1b,
            0xc0..=0xc3 | 0xc8..=0xcb | 0xd0..=0xd3 | 0xd8..=0xdb,
        ]
    );
    if !canonical
        || instruction.code_size() != CodeSize::Code32
        || !matches!(
            instruction.code(),
            Code::Reservednop_rm16_r16_0F1A | Code::Reservednop_rm16_r16_0F1B
        )
        || instruction.op_count() != 2
        || instruction.op0_kind() != OpKind::Register
        || instruction.op1_kind() != OpKind::Register
    {
        return false;
    }
    !CPU_ID_LEAVES.iter().any(|leaf| {
        leaf.function == 7 && leaf.subfunction == Some(0) && leaf.registers[1] & (1 << 14) != 0
    })
}

#[cfg(test)]
mod tests;
