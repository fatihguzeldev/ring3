use iced_x86::{Code, Instruction, Mnemonic};

use super::{Cpu32, GuestMemory, StopReason, result_flags};

impl Cpu32 {
    pub(super) fn shift(
        &mut self,
        instruction: &Instruction,
        memory: &mut GuestMemory,
    ) -> Result<(), StopReason> {
        let destination = self.operand(instruction, 0)?;
        let original = self.read_operand(destination, memory)?;
        let count = self.read_operand(self.operand(instruction, 1)?, memory)? & 31;
        let width = destination.width;
        let operation = instruction.mnemonic();
        let mut result = original;
        let mut carry = false;
        for _ in 0..count {
            if operation == Mnemonic::Shl {
                carry = result & width.sign_bit() != 0;
                result = (result << 1) & width.mask();
            } else {
                carry = result & 1 != 0;
                let sign = if operation == Mnemonic::Sar {
                    result & width.sign_bit()
                } else {
                    0
                };
                result = (result >> 1) | sign;
            }
        }
        // a zero count still checks memory read/write access.
        self.write_operand(destination, result, memory)?;
        if count == 0 {
            return Ok(());
        }
        // undefined flags are zeroed deterministically.
        if operation != Mnemonic::Sar && count >= width as u32 * 8 {
            carry = false;
        }
        let overflow = count == 1
            && match operation {
                Mnemonic::Shl => (result & width.sign_bit() != 0) != carry,
                Mnemonic::Shr => original & width.sign_bit() != 0,
                _ => false,
            };
        self.eflags = (self.eflags & !0x8d5)
            | result_flags(result, width)
            | u32::from(carry)
            | (u32::from(overflow) << 11);
        Ok(())
    }
}

pub(super) fn is_shift(code: Code) -> bool {
    matches!(
        code,
        Code::Shl_rm8_1
            | Code::Shl_rm16_1
            | Code::Shl_rm32_1
            | Code::Shl_rm8_CL
            | Code::Shl_rm16_CL
            | Code::Shl_rm32_CL
            | Code::Shl_rm8_imm8
            | Code::Shl_rm16_imm8
            | Code::Shl_rm32_imm8
            | Code::Shr_rm8_1
            | Code::Shr_rm16_1
            | Code::Shr_rm32_1
            | Code::Shr_rm8_CL
            | Code::Shr_rm16_CL
            | Code::Shr_rm32_CL
            | Code::Shr_rm8_imm8
            | Code::Shr_rm16_imm8
            | Code::Shr_rm32_imm8
            | Code::Sar_rm8_1
            | Code::Sar_rm16_1
            | Code::Sar_rm32_1
            | Code::Sar_rm8_CL
            | Code::Sar_rm16_CL
            | Code::Sar_rm32_CL
            | Code::Sar_rm8_imm8
            | Code::Sar_rm16_imm8
            | Code::Sar_rm32_imm8
    )
}
