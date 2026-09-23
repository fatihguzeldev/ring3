use iced_x86::{Code, Instruction};

use super::operands::Location;
use super::{Cpu32, GuestMemory, MemoryError, StopReason};

mod data;
mod rounding;
pub(super) use data::Stack;

impl Cpu32 {
    pub(super) fn x87_control(
        &mut self,
        instruction: &Instruction,
        memory: &mut GuestMemory,
    ) -> Result<(), StopReason> {
        let Location::Memory(address) = self.operand(instruction, 0)?.location else {
            return Err(StopReason::UnsupportedInstruction);
        };
        address
            .checked_add(1)
            .ok_or(StopReason::MemoryFault(MemoryError::AddressOverflow))?;
        if instruction.code() == Code::Fldcw_m2byte {
            let mut bytes = [0; 2];
            memory
                .read(u64::from(address), &mut bytes)
                .map_err(StopReason::MemoryFault)?;
            self.x87_control_word = u16::from_le_bytes(bytes);
        } else {
            memory
                .write(u64::from(address), &self.x87_control_word.to_le_bytes())
                .map_err(StopReason::MemoryFault)?;
        }
        Ok(())
    }
}

pub(super) fn is_arithmetic(code: Code) -> bool {
    matches!(
        code,
        Code::Fsqrt
            | Code::Fdiv_m32fp
            | Code::Fdiv_m64fp
            | Code::Fdivr_m32fp
            | Code::Fdivr_m64fp
            | Code::Fmul_m32fp
            | Code::Fmul_m64fp
            | Code::Fimul_m32int
            | Code::Fadd_m32fp
            | Code::Fadd_m64fp
            | Code::Fsub_m32fp
            | Code::Fsub_m64fp
            | Code::Fsubr_m32fp
            | Code::Fsubr_m64fp
    )
}
