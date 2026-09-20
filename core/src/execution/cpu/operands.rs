use iced_x86::{Instruction, MemorySize, OpKind, Register};

use super::{Cpu32, GuestMemory, MemoryError, Register32, StopReason, register32};

#[derive(Clone, Copy)]
pub(super) enum Width {
    Byte = 1,
    Word = 2,
    Dword = 4,
}

impl Width {
    pub(super) fn mask(self) -> u32 {
        u32::MAX >> (32 - self as u32 * 8)
    }

    pub(super) fn sign_bit(self) -> u32 {
        1 << (self as u32 * 8 - 1)
    }
}

#[derive(Clone, Copy)]
pub(super) enum Location {
    Register { parent: Register32, shift: u32 },
    Memory(u32),
    Immediate(u32),
}

#[derive(Clone, Copy)]
pub(super) struct Operand {
    pub(super) location: Location,
    pub(super) width: Width,
}

impl Cpu32 {
    pub(super) fn operand(
        &self,
        instruction: &Instruction,
        index: u32,
    ) -> Result<Operand, StopReason> {
        let immediate = |value, width| Operand {
            location: Location::Immediate(value),
            width,
        };
        match instruction.op_kind(index) {
            OpKind::Register => register_view(instruction.op_register(index)),
            OpKind::Memory => {
                let offset = self.effective_address(instruction)?;
                let base = if instruction.memory_segment() == Register::FS {
                    self.fs_base
                } else {
                    0
                };
                let width = match instruction.memory_size() {
                    MemorySize::UInt8 => Width::Byte,
                    MemorySize::UInt16 => Width::Word,
                    MemorySize::UInt32 | MemorySize::DwordOffset => Width::Dword,
                    _ => return Err(StopReason::UnsupportedInstruction),
                };
                Ok(Operand {
                    location: Location::Memory(base.wrapping_add(offset)),
                    width,
                })
            }
            OpKind::Immediate8 => Ok(immediate(u32::from(instruction.immediate8()), Width::Byte)),
            OpKind::Immediate16 => Ok(immediate(u32::from(instruction.immediate16()), Width::Word)),
            OpKind::Immediate32 => Ok(immediate(instruction.immediate32(), Width::Dword)),
            OpKind::Immediate8to16 => Ok(immediate(
                u32::from(instruction.immediate8to16().cast_unsigned()),
                Width::Word,
            )),
            OpKind::Immediate8to32 => Ok(immediate(
                instruction.immediate8to32().cast_unsigned(),
                Width::Dword,
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
        operand: Operand,
        memory: &GuestMemory,
    ) -> Result<u32, StopReason> {
        match operand.location {
            Location::Register { parent, shift } => {
                Ok((self.register(parent) >> shift) & operand.width.mask())
            }
            Location::Immediate(value) => Ok(value),
            Location::Memory(address) => read_value(memory, address, operand.width),
        }
    }

    pub(super) fn write_operand(
        &mut self,
        operand: Operand,
        value: u32,
        memory: &mut GuestMemory,
    ) -> Result<(), StopReason> {
        match operand.location {
            Location::Register { parent, shift } => {
                let mask = operand.width.mask();
                self.set_register(
                    parent,
                    (self.register(parent) & !(mask << shift)) | ((value & mask) << shift),
                );
            }
            Location::Memory(address) => write_value(memory, address, operand.width, value)?,
            Location::Immediate(_) => return Err(StopReason::UnsupportedInstruction),
        }
        Ok(())
    }
}

fn register_view(register: Register) -> Result<Operand, StopReason> {
    let (parent, width, shift) = match register {
        Register::AL => (Register32::Eax, Width::Byte, 0),
        Register::CL => (Register32::Ecx, Width::Byte, 0),
        Register::DL => (Register32::Edx, Width::Byte, 0),
        Register::BL => (Register32::Ebx, Width::Byte, 0),
        Register::AH => (Register32::Eax, Width::Byte, 8),
        Register::CH => (Register32::Ecx, Width::Byte, 8),
        Register::DH => (Register32::Edx, Width::Byte, 8),
        Register::BH => (Register32::Ebx, Width::Byte, 8),
        Register::AX => (Register32::Eax, Width::Word, 0),
        Register::CX => (Register32::Ecx, Width::Word, 0),
        Register::DX => (Register32::Edx, Width::Word, 0),
        Register::BX => (Register32::Ebx, Width::Word, 0),
        Register::SP => (Register32::Esp, Width::Word, 0),
        Register::BP => (Register32::Ebp, Width::Word, 0),
        Register::SI => (Register32::Esi, Width::Word, 0),
        Register::DI => (Register32::Edi, Width::Word, 0),
        _ => (register32(register)?, Width::Dword, 0),
    };
    Ok(Operand {
        location: Location::Register { parent, shift },
        width,
    })
}

fn read_value(memory: &GuestMemory, address: u32, width: Width) -> Result<u32, StopReason> {
    check_span(address, width)?;
    let mut bytes = [0; 4];
    memory
        .read(u64::from(address), &mut bytes[..width as usize])
        .map_err(StopReason::MemoryFault)?;
    Ok(u32::from_le_bytes(bytes))
}

fn write_value(
    memory: &mut GuestMemory,
    address: u32,
    width: Width,
    value: u32,
) -> Result<(), StopReason> {
    check_span(address, width)?;
    memory
        .write(u64::from(address), &value.to_le_bytes()[..width as usize])
        .map_err(StopReason::MemoryFault)
}

fn check_span(address: u32, width: Width) -> Result<(), StopReason> {
    address
        .checked_add(width as u32 - 1)
        .ok_or(StopReason::MemoryFault(MemoryError::AddressOverflow))?;
    Ok(())
}

pub(super) fn read_dword(memory: &GuestMemory, address: u32) -> Result<u32, StopReason> {
    read_value(memory, address, Width::Dword)
}

pub(super) fn write_dword(
    memory: &mut GuestMemory,
    address: u32,
    value: u32,
) -> Result<(), StopReason> {
    write_value(memory, address, Width::Dword, value)
}
