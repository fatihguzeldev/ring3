use iced_x86::Code;

use super::operands::{Width, read_value, write_value};
use super::{Cpu32, GuestMemory, Register32, StopReason};

impl Cpu32 {
    pub(super) fn flag_stack(
        &mut self,
        code: Code,
        memory: &mut GuestMemory,
    ) -> Result<(), StopReason> {
        // virtual-8086, single-step and alignment traps have no execution path.
        if self.eflags & 0x0006_0100 != 0 {
            return Err(StopReason::UnsupportedInstruction);
        }
        let width = if matches!(code, Code::Pushfw | Code::Popfw) {
            Width::Word
        } else {
            Width::Dword
        };
        let stack = self.register(Register32::Esp);
        if matches!(code, Code::Pushfw | Code::Pushfd) {
            let address = stack.wrapping_sub(width as u32);
            write_value(memory, address, width, self.eflags & 0x00fc_ffff)?;
            self.set_register(Register32::Esp, address);
        } else {
            let value = read_value(memory, stack, width)?;
            let mut writable = 0x0000_4dd5;
            if matches!(width, Width::Dword) {
                writable |= 0x0024_0000;
            }
            if value & writable & 0x0004_0100 != 0 {
                return Err(StopReason::UnsupportedInstruction);
            }
            if self.eflags & 0x3000 == 0x3000 {
                writable |= 0x200;
            }
            self.eflags = (self.eflags & !(writable | 0x10000)) | (value & writable);
            self.set_register(Register32::Esp, stack.wrapping_add(width as u32));
        }
        Ok(())
    }
}
