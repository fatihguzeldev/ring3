use iced_x86::{Code, Instruction, OpKind, Register};

use super::operands::{Location, Operand, Width};
use super::{Cpu32, GuestMemory, Register32, StopReason};

impl Cpu32 {
    pub(super) fn move_string(
        &mut self,
        instruction: &Instruction,
        memory: &mut GuestMemory,
    ) -> Result<(), StopReason> {
        if instruction.op0_kind() != OpKind::MemoryESEDI
            || instruction.op1_kind() != OpKind::MemorySegESI
        {
            return Err(StopReason::UnsupportedInstruction);
        }
        let width = match instruction.code() {
            Code::Movsb_m8_m8 => Width::Byte,
            Code::Movsw_m16_m16 => Width::Word,
            Code::Movsd_m32_m32 => Width::Dword,
            _ => return Err(StopReason::UnsupportedInstruction),
        };
        let source = self.register(Register32::Esi);
        let destination = self.register(Register32::Edi);
        let source_base = if instruction.segment_prefix() == Register::FS {
            self.fs_base
        } else {
            0
        };
        let operand = |address| Operand {
            location: Location::Memory(address),
            width,
        };
        let value = self.read_operand(operand(source_base.wrapping_add(source)), memory)?;
        self.write_operand(operand(destination), value, memory)?;
        let step = if self.eflags & 0x400 == 0 {
            width as u32
        } else {
            (width as u32).wrapping_neg()
        };
        self.set_register(Register32::Esi, source.wrapping_add(step));
        self.set_register(Register32::Edi, destination.wrapping_add(step));
        Ok(())
    }
}
