use iced_x86::{Code, Instruction};

use super::operands::Operand32;
use super::{Cpu32, GuestMemory, MemoryError, StopReason};

impl Cpu32 {
    pub(super) fn x87_control(
        &mut self,
        instruction: &Instruction,
        memory: &mut GuestMemory,
    ) -> Result<(), StopReason> {
        let Operand32::Memory(address) = self.operand(instruction, 0)? else {
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
