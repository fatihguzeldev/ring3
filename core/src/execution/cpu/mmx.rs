use iced_x86::{Code, Instruction, OpKind, Register};

use super::{Cpu32, GuestMemory, MemoryError, StopReason};

pub(super) fn is_transfer(code: Code) -> bool {
    matches!(
        code,
        Code::Movd_mm_rm32
            | Code::Movd_rm32_mm
            | Code::Movq_mm_mmm64
            | Code::Movq_mmm64_mm
            | Code::Emms
    )
}

impl Cpu32 {
    pub(super) fn mmx_transfer(
        &mut self,
        instruction: &Instruction,
        memory: &mut GuestMemory,
    ) -> Result<(), StopReason> {
        self.x87_stack.mmx_ready(self.x87_control_word)?;
        match instruction.code() {
            Code::Emms => {
                self.x87_stack.mmx_empty();
                return Ok(());
            }
            Code::Movd_mm_rm32 => {
                let value = self.read_operand(self.operand(instruction, 1)?, memory)?;
                self.x87_stack
                    .mmx_write(mmx_index(instruction.op0_register())?, u64::from(value));
            }
            Code::Movd_rm32_mm => {
                let value = self
                    .x87_stack
                    .mmx_read(mmx_index(instruction.op1_register())?);
                self.write_operand(
                    self.operand(instruction, 0)?,
                    u32::try_from(value & u64::from(u32::MAX)).expect("low dword"),
                    memory,
                )?;
            }
            Code::Movq_mm_mmm64 => {
                let value = if instruction.op1_kind() == OpKind::Register {
                    self.x87_stack
                        .mmx_read(mmx_index(instruction.op1_register())?)
                } else {
                    let mut bytes = [0; 8];
                    memory
                        .read(u64::from(self.mmx_address(instruction)?), &mut bytes)
                        .map_err(StopReason::MemoryFault)?;
                    u64::from_le_bytes(bytes)
                };
                self.x87_stack
                    .mmx_write(mmx_index(instruction.op0_register())?, value);
            }
            Code::Movq_mmm64_mm => {
                let value = self
                    .x87_stack
                    .mmx_read(mmx_index(instruction.op1_register())?);
                if instruction.op0_kind() == OpKind::Register {
                    self.x87_stack
                        .mmx_write(mmx_index(instruction.op0_register())?, value);
                } else {
                    memory
                        .write(
                            u64::from(self.mmx_address(instruction)?),
                            &value.to_le_bytes(),
                        )
                        .map_err(StopReason::MemoryFault)?;
                }
            }
            _ => return Err(StopReason::UnsupportedInstruction),
        }
        self.x87_stack.mmx_enter();
        Ok(())
    }

    fn mmx_address(&self, instruction: &Instruction) -> Result<u32, StopReason> {
        let base = if instruction.memory_segment() == Register::FS {
            self.fs_base
        } else {
            0
        };
        let address = base.wrapping_add(self.effective_address(instruction)?);
        address
            .checked_add(7)
            .ok_or(StopReason::MemoryFault(MemoryError::AddressOverflow))?;
        Ok(address)
    }
}

fn mmx_index(register: Register) -> Result<usize, StopReason> {
    let index = (register as usize).wrapping_sub(Register::MM0 as usize);
    if index < 8 {
        Ok(index)
    } else {
        Err(StopReason::UnsupportedInstruction)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::{PAGE_SIZE, Permissions};

    #[test]
    fn crt_helpers_refuse_packed_values_until_emms() {
        let mut memory = GuestMemory::new(1);
        memory
            .map_zeroed(0x1000, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
        memory
            .write(0x1000, &[0x0f, 0x6e, 0xc0, 0x0f, 0x77])
            .unwrap();
        memory
            .protect(0x1000, PAGE_SIZE, Permissions::READ_EXECUTE)
            .unwrap();
        let mut cpu = Cpu32::new(0x1000);
        assert_eq!(cpu.run(&mut memory, 1).reason, StopReason::InstructionLimit);
        let before = cpu;
        assert_eq!(cpu.push_x87_double(1.0), None);
        assert_eq!(cpu.pop_x87_truncated_integer(), None);
        assert_eq!(cpu, before);
        assert_eq!(cpu.run(&mut memory, 1).reason, StopReason::InstructionLimit);
        assert_eq!(cpu.push_x87_double(42.5), Some(()));
        assert_eq!(cpu.pop_x87_truncated_integer(), Some(42));
    }
}
