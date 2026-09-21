use iced_x86::{Code, Instruction, OpKind, Register};

use super::operands::{Location, Operand, Width};
use super::{Cpu32, GuestMemory, Register32, StopReason};

pub(super) fn is_scan(code: Code) -> bool {
    matches!(
        code,
        Code::Scasb_AL_m8 | Code::Scasw_AX_m16 | Code::Scasd_EAX_m32
    )
}

impl Cpu32 {
    pub(super) fn scan_string(
        &mut self,
        instruction: &Instruction,
        memory: &GuestMemory,
    ) -> Result<u32, StopReason> {
        if instruction.op1_kind() != OpKind::MemoryESEDI {
            return Err(StopReason::UnsupportedInstruction);
        }
        let repeat = instruction.has_rep_prefix() || instruction.has_repne_prefix();
        let count = self.register(Register32::Ecx);
        if repeat && count == 0 {
            return Ok(instruction.next_ip32());
        }
        let width = match instruction.code() {
            Code::Scasb_AL_m8 => Width::Byte,
            Code::Scasw_AX_m16 => Width::Word,
            Code::Scasd_EAX_m32 => Width::Dword,
            _ => return Err(StopReason::UnsupportedInstruction),
        };
        let address = self.register(Register32::Edi);
        let left = self.register(Register32::Eax) & width.mask();
        let right = self.read_operand(
            Operand {
                location: Location::Memory(address),
                width,
            },
            memory,
        )?;
        let step = if self.eflags & 0x400 == 0 {
            width as u32
        } else {
            (width as u32).wrapping_neg()
        };
        self.set_register(Register32::Edi, address.wrapping_add(step));
        if repeat {
            self.set_register(Register32::Ecx, count - 1);
            let keep_scanning = if instruction.has_repne_prefix() {
                left != right
            } else {
                left == right
            };
            if count > 1 && keep_scanning {
                // defer flags so a later scan fault retains pre-instruction eflags.
                return Ok(self.eip);
            }
        }
        self.arithmetic_flags(
            left,
            right,
            left.wrapping_sub(right) & width.mask(),
            left < right,
            true,
            width,
        );
        Ok(instruction.next_ip32())
    }

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
