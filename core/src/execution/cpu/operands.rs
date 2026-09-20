use iced_x86::{Instruction, OpKind, Register};

use super::{Cpu32, GuestMemory, MemoryError, Register32, StopReason, register32};

#[derive(Clone, Copy)]
pub(super) enum Operand32 {
    Register(Register32),
    Memory(u32),
    Immediate(u32),
}

impl Cpu32 {
    pub(super) fn operand(
        &self,
        instruction: &Instruction,
        index: u32,
    ) -> Result<Operand32, StopReason> {
        match instruction.op_kind(index) {
            OpKind::Register => Ok(Operand32::Register(register32(
                instruction.op_register(index),
            )?)),
            OpKind::Memory => {
                let offset = self.effective_address(instruction)?;
                let base = if instruction.memory_segment() == Register::FS {
                    self.fs_base
                } else {
                    0
                };
                Ok(Operand32::Memory(base.wrapping_add(offset)))
            }
            OpKind::Immediate32 => Ok(Operand32::Immediate(instruction.immediate32())),
            OpKind::Immediate8to32 => Ok(Operand32::Immediate(
                instruction.immediate8to32().cast_unsigned(),
            )),
            _ => Err(StopReason::UnsupportedInstruction),
        }
    }

    pub(super) fn effective_address(&self, instruction: &Instruction) -> Result<u32, StopReason> {
        if instruction.memory_displ_size() == 2 {
            return Err(StopReason::UnsupportedInstruction);
        }
        let value = |register| {
            if register == Register::None {
                Ok(0)
            } else {
                register32(register).map(|reg| self.register(reg))
            }
        };
        Ok(value(instruction.memory_base())?
            .wrapping_add(
                value(instruction.memory_index())?.wrapping_mul(instruction.memory_index_scale()),
            )
            .wrapping_add(instruction.memory_displacement32()))
    }

    pub(super) fn read_operand(
        &self,
        operand: Operand32,
        memory: &GuestMemory,
    ) -> Result<u32, StopReason> {
        match operand {
            Operand32::Register(register) => Ok(self.register(register)),
            Operand32::Immediate(value) => Ok(value),
            Operand32::Memory(address) => read_dword(memory, address),
        }
    }

    pub(super) fn write_operand(
        &mut self,
        operand: Operand32,
        value: u32,
        memory: &mut GuestMemory,
    ) -> Result<(), StopReason> {
        match operand {
            Operand32::Register(register) => self.set_register(register, value),
            Operand32::Memory(address) => write_dword(memory, address, value)?,
            Operand32::Immediate(_) => return Err(StopReason::UnsupportedInstruction),
        }
        Ok(())
    }
}

pub(super) fn read_dword(memory: &GuestMemory, address: u32) -> Result<u32, StopReason> {
    check_dword(address)?;
    let mut bytes = [0; 4];
    memory
        .read(u64::from(address), &mut bytes)
        .map_err(StopReason::MemoryFault)?;
    Ok(u32::from_le_bytes(bytes))
}

pub(super) fn write_dword(
    memory: &mut GuestMemory,
    address: u32,
    value: u32,
) -> Result<(), StopReason> {
    check_dword(address)?;
    memory
        .write(u64::from(address), &value.to_le_bytes())
        .map_err(StopReason::MemoryFault)
}

fn check_dword(address: u32) -> Result<(), StopReason> {
    address
        .checked_add(3)
        .ok_or(StopReason::MemoryFault(MemoryError::AddressOverflow))?;
    Ok(())
}
